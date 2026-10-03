//! 重建向量索引：把**库里已有的载荷**重新算一遍向量，并在过程中报进度。
//!
//! 为什么必须有它：向量不是自描述的。换了 embedding 模型或改了维度之后，库里那些
//! 旧向量就再也不能用了 —— 两个模型的向量空间毫不相干（检索**不报错**，只是给出
//! 噪声），维度不同的话 `vector_distance_cos` 会让整条查询直接失败。唯一的修法就是
//! 拿原文重算一遍。载荷都还在，所以**不需要重新上传任何东西**：
//!
//! | 表 | 载荷 | 重建入口 |
//! | --- | --- | --- |
//! | 意图短语 `phrase.dat` | 红库 `{robot_id}intents` 里的 `phrases[].phrase` | [`crate::intent::phrase::reindex`] |
//! | 问答 `qa.dat` | `{robot_id}.qa_data` 里的主问题 + 相似问题 | [`crate::kb::qa::reindex`] |
//! | 文档 `doc.dat` | `{robot_id}.doc_content` | [`crate::kb::doc::reindex`] |
//!
//! ## 为什么是后台任务 + 轮询
//!
//! 一份长文档就是几百个 chunk，每个都要一次推理/一次远端调用。放在请求里同步做，
//! 前端只能一直转圈，而且中途断网就没人知道做到哪了。这里沿用设置页那套
//! [`crate::man::settings::model_load_progress`] 的形状：`POST` 立刻返回，进度写进
//! [`REINDEX_STATUS`]，前端每秒轮询。
//!
//! ## 重建期间库里是**混合**的
//!
//! 逐条重写没有全局原子性：重建过程的中途，表里同时存在新向量和旧向量。此时检索
//! 要么报错（维度不同）要么给出噪声。这是"宁可短暂不可用，也不要一直不可用"的取舍；
//! 前端在重建期间应当明确告知用户先不要用知识库。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use axum::extract::Query;
use axum::response::IntoResponse;
use serde::Serialize;

use crate::db;
use crate::db_executor;
use crate::intent::crud as intent_crud;
use crate::intent::dto::IntentDetail;
use crate::kb::{doc, qa};
use crate::man::settings;
use crate::result::{Error, Result};
use crate::robot::dto::RobotQuery;
use crate::web::server::to_res;

/// 重建进行到哪一类数据。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReindexPhase {
    /// 还没开始（`start` 刚记录状态，后台任务还没跑起来）。
    Starting,
    Intents,
    Qa,
    Docs,
    Done,
    Failed,
}

impl ReindexPhase {
    pub(crate) fn is_running(self) -> bool {
        matches!(
            self,
            ReindexPhase::Starting | ReindexPhase::Intents | ReindexPhase::Qa | ReindexPhase::Docs
        )
    }
}

/// 单类数据的进度。
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceProgress {
    /// 已经重建完的条数（意图是"短语条数"，问答是"问答对条数"，文档是"chunk 数"）。
    pub(crate) done: usize,
    /// 打算重建的总条数。开跑前是 0，所以界面要能显示"统计中"。
    pub(crate) total: usize,
    /// 这一类**已经有**的向量行数（重建前统计一次，用来展示"从 N 条变成 M 条"）。
    pub(crate) before: usize,
    /// 重建后实际的向量行数。
    pub(crate) after: usize,
}

impl SourceProgress {
    fn begin(&mut self, total: usize, before: usize) {
        self.total = total;
        self.before = before;
        self.done = 0;
        self.after = 0;
    }
}

/// 一次重建的完整状态。没有记录过的机器人返回 `Default`（`phase = Idle` 那种），
/// 所以前端不必区分"没跑过"和"刚跑完"——这一点和 `model_load_progress` 一致。
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReindexStatus {
    pub(crate) phase: Option<ReindexPhase>,
    pub(crate) intents: SourceProgress,
    pub(crate) qa: SourceProgress,
    pub(crate) docs: SourceProgress,
    /// 失败原因，原样给前端弹出来（哪个阶段、为什么）。成功时为空串。
    pub(crate) err: String,
    /// 开始时间（unix 秒），方便界面显示"已经跑了多久"。
    pub(crate) started_at: u64,
    pub(crate) finished_at: u64,
}

impl ReindexStatus {
    fn running(&self) -> bool {
        self.phase.map(ReindexPhase::is_running).unwrap_or(false)
    }
}

static REINDEX_STATUS: LazyLock<Mutex<HashMap<String, ReindexStatus>>> =
    LazyLock::new(|| Mutex::new(HashMap::with_capacity(8)));

/// 读一份状态（加锁失败就当没有）。
fn status_of(robot_id: &str) -> ReindexStatus {
    REINDEX_STATUS
        .lock()
        .ok()
        .and_then(|m| m.get(robot_id).cloned())
        .unwrap_or_default()
}

/// 改一份状态；每处改动都要连带置 `phase`，否则前端会卡在上一阶段。
fn update_status<F: FnOnce(&mut ReindexStatus)>(robot_id: &str, f: F) {
    if let Ok(mut m) = REINDEX_STATUS.lock() {
        let s = m.entry(String::from(robot_id)).or_default();
        f(s);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 数一张表有多少行；表不存在（还没写过这一类数据）返回 0。
async fn count_rows(file: &str, table: &str) -> usize {
    crate::man::vector_report::count_rows(file, table)
        .await
        .unwrap_or(0)
}

/// 真正干活的那段。**必须在后台任务里跑**（[`start`] 负责 spawn）。
async fn run(robot_id: &str) -> Result<()> {
    // 0. 先确认当前配置真的能算向量。一个空机器人（三类数据都没有）此前会"成功地
    //    什么都没做"，用户完全看不出模型是坏的；这里用一句话换来确定性。
    crate::ai::embedding::embedding(robot_id, "reindex probe").await?;

    // 1. 意图短语。
    //
    //    先**清空整张短语向量表**再重建：重建只负责"给现存的数据写新向量"，不会
    //    删掉已经不存在的短语留下的孤儿行。孤儿行的维度和新配置不一致，会让
    //    `vector_distance_cos` 在扫到它时报错——那正是我们要修的问题，所以必须连
    //    它们一起清掉。短语表只有向量列（载荷在红库里），可以整表重建。
    //
    //    总数要先把所有意图的短语加起来，否则界面上的进度条没有分母。
    let intents: Vec<IntentDetail> =
        db_executor!(db::get_all, robot_id, intent_crud::TABLE_SUFFIX,)?;
    let total_phrases: usize = intents.iter().map(|d| d.phrases.len()).sum();
    let before_intents = count_rows("phrase.dat", robot_id).await;
    update_status(robot_id, |s| {
        s.phase = Some(ReindexPhase::Intents);
        s.intents.begin(total_phrases, before_intents);
    });
    crate::intent::phrase::remove_tables(robot_id).await?;
    crate::intent::phrase::reindex(robot_id, &intents).await?;
    update_status(robot_id, |s| s.intents.done = total_phrases);
    let after_intents = count_rows("phrase.dat", robot_id).await;
    update_status(robot_id, |s| s.intents.after = after_intents);

    // 2. 问答。同理先清空 `{robot_id}_vec`：一次 `qa::save` 会重算主问题 + 全部
    //    相似问题，并把 `qa_data` 里的 `vec_row_id` 刷成新行号，但删掉过的问答对
    //    留下的孤儿向量它管不到。**只删 `_vec`**，主表（`qa_data`）是载荷，不能动。
    let pairs = qa::list(robot_id).await?;
    let before_qa = count_rows("qa.dat", &format!("{robot_id}_vec")).await;
    update_status(robot_id, |s| {
        s.phase = Some(ReindexPhase::Qa);
        s.qa.begin(pairs.len(), before_qa);
    });
    qa::remove_vector_table(robot_id).await?;
    for p in pairs {
        qa::save(robot_id, p).await?;
        update_status(robot_id, |s| s.qa.done += 1);
    }
    let after_qa = count_rows("qa.dat", &format!("{robot_id}_vec")).await;
    update_status(robot_id, |s| s.qa.after = after_qa);

    // 3. 文档。按"文档"计数（不是按 chunk），`doc::reindex` 内部会把该文档的分块
    //    全部重算并在同一个事务里替换。
    let docs = doc::list(robot_id).await?;
    let before_docs = count_rows("doc.dat", &format!("{robot_id}_vec")).await;
    update_status(robot_id, |s| {
        s.phase = Some(ReindexPhase::Docs);
        s.docs.begin(docs.len(), before_docs);
    });
    for d in docs.iter() {
        doc::reindex(robot_id, d.id).await?;
        update_status(robot_id, |s| s.docs.done += 1);
    }
    let after_docs = count_rows("doc.dat", &format!("{robot_id}_vec")).await;
    update_status(robot_id, |s| s.docs.after = after_docs);

    // 4. 定稿：从现在起库里那份索引就是当前配置算的了（设置页的"模型/维度变了"
    //    警告靠这个标记来消）。放在**全部成功之后**——中途失败时标不能被改，
    //    否则用户会以为已经修好了。
    settings::stamp_embedding_index(robot_id).await?;
    Ok(())
}

/// `POST /management/settings/embedding/reindex`：开始重建，立刻返回。
///
/// 同一个机器人同时只允许一个重建任务：两个任务并发会同时重写同一批行，除了浪费
/// 推理什么都不多得到。
pub(crate) async fn start(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    let robot_id = q.robot_id;
    if status_of(&robot_id).running() {
        return to_res(Err(Error::WithMessage(String::from(
            "lang.settings.reindexRunning",
        ))));
    }
    update_status(&robot_id, |s| {
        *s = ReindexStatus {
            phase: Some(ReindexPhase::Starting),
            started_at: now_secs(),
            ..ReindexStatus::default()
        };
    });
    let rid = robot_id.clone();
    tokio::spawn(async move {
        let r = run(&rid).await;
        let err = match r {
            Ok(()) => String::new(),
            Err(e) => e.message(),
        };
        if !err.is_empty() {
            log::warn!("Reindexing {rid} failed: {err}");
        }
        update_status(&rid, |s| {
            s.finished_at = now_secs();
            if err.is_empty() {
                s.phase = Some(ReindexPhase::Done);
            } else {
                s.phase = Some(ReindexPhase::Failed);
                s.err = err;
            }
        });
    });
    to_res(Ok(()))
}

/// `GET /management/settings/embedding/reindex/progress`。
pub(crate) async fn progress(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    to_res(Ok(status_of(&q.robot_id)))
}

/// 给前端用的"要不要重建"的提示：当前配置 vs 库里那份索引。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReindexHint {
    /// 库里那份索引的指纹（`v1|kind|model|dims`），从没写过向量时为 `null`。
    pub(crate) indexed: Option<String>,
    /// 当前配置对应的指纹。
    pub(crate) configured: String,
}

pub(crate) async fn hint(Query(q): Query<RobotQuery>) -> impl IntoResponse {
    let r = async {
        let s = settings::get_settings(&q.robot_id).await?.ok_or_else(|| {
            Error::WithMessage(format!("Can not find settings of {}", q.robot_id))
        })?;
        Ok(ReindexHint {
            indexed: s.sentence_embedding_provider.indexed_embedding.clone(),
            configured: s.current_embedding_identity(),
        })
    }
    .await;
    to_res(r)
}

/// 只给测试用：清掉某个机器人的状态。
#[cfg(test)]
pub(crate) fn clear_status(robot_id: &str) {
    if let Ok(mut m) = REINDEX_STATUS.lock() {
        m.remove(robot_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 状态机的关键性质：只有"跑到一半"的四个阶段算运行中，`Done` / `Failed` /
    /// 没记录过都不算——否则重建失败之后用户再也点不动那个按钮。
    #[test]
    fn only_in_flight_phases_count_as_running() {
        for p in [
            ReindexPhase::Starting,
            ReindexPhase::Intents,
            ReindexPhase::Qa,
            ReindexPhase::Docs,
        ] {
            assert!(p.is_running(), "{p:?} 应该在运行中");
        }
        for p in [ReindexPhase::Done, ReindexPhase::Failed] {
            assert!(!p.is_running(), "{p:?} 不应该算运行中");
        }
        assert!(!ReindexStatus::default().running(), "没记录过 = 不在跑");
    }

    /// `SourceProgress::begin` 要能把上一轮跑剩的计数清干净：同一个机器人重建两次，
    /// 第二次的 `done` 不能从第一次的数字接着涨。
    #[test]
    fn begin_resets_leftover_counters() {
        let mut p = SourceProgress {
            done: 42,
            total: 99,
            before: 7,
            after: 7,
        };
        p.begin(3, 5);
        assert_eq!(p.done, 0);
        assert_eq!(p.total, 3);
        assert_eq!(p.before, 5);
        assert_eq!(p.after, 0, "after 要等本轮跑完才填");
    }

    /// 默认状态是"没跑过"，前端不必区分"没跑过"和"刚跑完"。
    #[test]
    fn a_robot_without_a_record_reports_a_default_status() {
        clear_status("no-such-robot-in-this-test");
        let s = status_of("no-such-robot-in-this-test");
        assert!(s.phase.is_none());
        assert!(!s.running());
        assert_eq!(s.intents.total, 0);
        assert_eq!(s.qa.done, 0);
        assert_eq!(s.err, "");
    }
}
