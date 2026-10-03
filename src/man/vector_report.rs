//! 诊断：把**已经存进库里的向量维度**报出来。
//!
//! 为什么需要它：设置页显示的 `dimensions` 是"**以后**要按几维去算"，不保证和库里
//! 已有的向量一致（改过设置、又还没重新索引时两者就是不一致的）。而 turso 的
//! `F32_BLOB` 只是"一串裸 f32"：**维度 = blob 字节数 / 4**，没有头部、没有维度字段
//! （见 turso_core 的 `vector/vector_types.rs`：`dims = data.len() / 4`）。
//!
//! 那 `PRAGMA table_info` 呢？能用，但**帮不上忙**（turso 0.8.1 实测）：
//! `F32_BLOB(3072)` 建表之后 `table_info` 回的是 `F32_BLOB`——维数在建表时就被丢掉
//! 了；另外 `PRAGMA table_info(?)` 这种带绑定的写法会语法报错，能用的是表值函数形态
//! `pragma_table_info(?)`。所以它只能回答"有没有这张表/这个列"，真实维度必须数 blob。
//! 这里两者都用：先用 table_info 分辨"表还没建"，再数长度给出真实维度。
//!
//! 表和列（三张表在不同文件里，见各自的 `init_datasource`）：
//!
//! | 文件 | 表 | 向量列 | 载荷在哪 |
//! | --- | --- | --- | --- |
//! | `data/qa.dat` | `{robot_id}_vec` | `qa_vec` | 主表 `{robot_id}.qa_data` 的 JSON |
//! | `data/qa.dat` | `{robot_id}`（意图短语） | `phrase_vec` | 同表的 `phrase` |
//! | `data/doc.dat` | `{robot_id}_vec` | `chunk_vec` | 同表的 `chunk_text` |
//!
//! 设置页「检测已存向量维度」按钮走 [`robot_vector_report`]（只读，见
//! `man::settings::vector_dimensions`）；命令行想直接看就跑
//!
//! ```text
//! cargo test report_vector_dimensions -- --nocapture
//! ```

use std::path::Path;
use std::vec::Vec;

use serde::Serialize;
use turso::{Connection, Database};

use crate::result::Result;

/// 一个机器人 + 一处向量列在库里的实际情况。
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VectorColumn {
    /// 数据文件，如 `qa.dat`。
    pub(crate) file: String,
    /// 表名，如 `{robot_id}_vec`。
    pub(crate) table: String,
    /// 向量列名，如 `qa_vec`。
    pub(crate) column: String,
    /// 人类可读的来源，形如 `data/qa.dat: {robot_id}_vec.qa_vec`。
    pub(crate) source: String,
    /// 表/列是否已经存在。**false 不是错误**：向量表是第一次写向量时才懒创建的
    /// （见 `kb::qa::save` / `kb::doc::save_doc_embedding`），"没建"就是"还没有向量"。
    pub(crate) exists: bool,
    /// 建表时写下的类型名，例如 `F32_BLOB`。
    ///
    /// **实测（turso 0.8.1）：维数在建表时就被丢掉了。** `F32_BLOB(3072)` 建表后，
    /// `PRAGMA table_info` 回的是 `F32_BLOB`；`PRAGMA table_info(?)` 这种带绑定的
    /// 写法还会直接语法报错（只认字面量或表值函数形态的 `pragma_table_info(?)`）。
    /// 所以它连"化石"都算不上，只是"这列是向量列"的标记——真实维度只能数 blob。
    pub(crate) declared_type: String,
    /// 行数。
    pub(crate) rows: usize,
    /// 出现过的维度（升序去重）。正常只有一种；**多于一种说明库里有混合维度**，
    /// 那是改维度时留下的一半新一半旧，检索会在 `vector_distance_cos` 上直接报错。
    pub(crate) dims: Vec<usize>,
}

/// 从 BLOB 长度反推维度。turso 的 f32 密集向量没有头部，就是这个关系
/// （`vector_types.rs` 里 `dims = data.len() / 4`）。
pub(crate) fn dims_of_blob(blob: &[u8]) -> usize {
    blob.len() / 4
}

/// 报告里的维度集合：空表 -> 空 `Vec`，正常 -> 单元素，混合维度 -> 多元素。
fn push_dim(dims: &mut Vec<usize>, d: usize) {
    if !dims.contains(&d) {
        dims.push(d);
        dims.sort_unstable();
    }
}

/// 要检查的一处向量列。
struct Target {
    file: &'static str,
    table: String,
    column: &'static str,
}

/// 打开只读连接。文件不存在返回 `None`（"这个机器人没有这类数据"）。
///
/// 只读是硬要求：这是诊断，绝不能碰用户的数据。
async fn open_read_only(path: &str) -> Option<Database> {
    if !Path::new(path).exists() {
        return None;
    }
    match turso::Builder::new_local(path).read_only(true).build().await {
        Ok(d) => Some(d),
        Err(e) => {
            log::warn!("Opening {path} read-only failed: {e}");
            None
        }
    }
}

/// 数一处向量列的维度。列不存在（表还没建、或这个表没有向量列）时 `exists = false`。
async fn inspect(conn: &Connection, t: &Target) -> VectorColumn {
    // `PRAGMA table_info` 支持参数绑定（turso 的 TableInfo 是表值函数形态）。
    // 它只能告诉我们"列在不在"和"建表时写的类型"，真实维度必须数 blob。
    let mut exists = false;
    let mut declared_type = String::new();
    if let Ok(mut rows) = conn
        .query("SELECT name, type FROM pragma_table_info(?)", (t.table.as_str(),))
        .await
    {
        while let Ok(Some(row)) = rows.next().await {
            let name = row.get_value(0).ok().and_then(|v| v.as_text().cloned());
            if name.as_deref() == Some(t.column) {
                exists = true;
                declared_type = row
                    .get_value(1)
                    .ok()
                    .and_then(|v| v.as_text().cloned())
                    .unwrap_or_default();
                break;
            }
        }
    }

    let mut dims: Vec<usize> = Vec::new();
    let mut n = 0usize;
    if exists {
        // 列名和表名都来自本文件的常量/注册表 id，不是用户输入。
        let sql = format!("SELECT {} FROM {}", t.column, t.table);
        match conn.query(sql.as_str(), ()).await {
            Ok(mut rows) => loop {
                match rows.next().await {
                    Ok(Some(row)) => {
                        let blob = row.get_value(0).ok().and_then(|v| v.as_blob().cloned());
                        let Some(blob) = blob else { continue };
                        n += 1;
                        push_dim(&mut dims, dims_of_blob(&blob));
                    }
                    Ok(None) => break,
                    Err(e) => {
                        log::warn!("Reading {} failed: {e}", t.table);
                        break;
                    }
                }
            },
            Err(e) => log::warn!("Querying {} failed: {e}", t.table),
        }
    }

    VectorColumn {
        file: String::from(t.file),
        table: t.table.clone(),
        column: String::from(t.column),
        source: format!("data/{}: {}.{}", t.file, t.table, t.column),
        exists,
        declared_type,
        rows: n,
        dims,
    }
}

/// 某个机器人在三个 `.dat` 里的向量维度。
///
/// 表名是 `{robot_id}` 拼出来的（见各 `init_datasource` 与建表处），所以这里也用
/// 字符串拼 SQL——和运行期完全一致。机器人 id 来自注册表，不是用户随手输入。
pub(crate) async fn robot_vector_report(robot_id: &str) -> Vec<VectorColumn> {
    let targets = [
        Target {
            file: "qa.dat",
            table: format!("{robot_id}_vec"),
            column: "qa_vec",
        },
        Target {
            file: "qa.dat",
            table: String::from(robot_id),
            column: "phrase_vec",
        },
        Target {
            file: "doc.dat",
            table: format!("{robot_id}_vec"),
            column: "chunk_vec",
        },
    ];

    let mut out: Vec<VectorColumn> = Vec::with_capacity(targets.len());
    // 同一个文件里的两处向量列共用一个连接（也共用一次 schema 读取）。
    let mut opened: Option<(String, Database)> = None;
    for t in targets.iter() {
        let need_open = opened.as_ref().map(|(f, _)| f.as_str()) != Some(t.file);
        if need_open {
            let path = format!("./data/{}", t.file);
            opened = open_read_only(&path).await.map(|d| (String::from(t.file), d));
        }
        let Some((file, db)) = opened.as_ref() else {
            // 文件不存在：这一类数据整个没有，直接给出 exists = false 的一条，
            // 让界面上三行始终都在（用户才知道"这三处都查过了"）。
            out.push(missing(t));
            continue;
        };
        debug_assert_eq!(file, t.file);
        match db.connect() {
            Ok(conn) => out.push(inspect(&conn, t).await),
            Err(e) => {
                log::warn!("Connecting {} failed: {e}", t.file);
                out.push(missing(t));
            }
        }
    }
    out
}

/// 文件打不开时给一条"还没建"的记录。
fn missing(t: &Target) -> VectorColumn {
    VectorColumn {
        file: String::from(t.file),
        table: t.table.clone(),
        column: String::from(t.column),
        source: format!("data/{}: {}.{}", t.file, t.table, t.column),
        exists: false,
        declared_type: String::new(),
        rows: 0,
        dims: Vec::new(),
    }
}

/// 数一张表有多少行，供重建索引用（`man::reindex` 展示"重建前有多少条"）。
///
/// **表不存在不是错误**：三张向量表都是懒创建的，没有数据时它根本不存在，此时答案是
/// 0。所以这里把"no such table"吞掉返回 0，只把真正打不开文件之类的错误往上报。
pub(crate) async fn count_rows(file: &str, table: &str) -> Result<usize> {
    let path = format!("./data/{file}");
    let Some(db) = open_read_only(&path).await else {
        return Ok(0);
    };
    let conn = db.connect()?;
    // 表名来自调用方的常量/注册表 id，不是用户输入；`robot_id` 是 scru128，无引号风险。
    let sql = format!("SELECT count(*) FROM {table}");
    let mut rows = match conn.query(sql.as_str(), ()).await {
        Ok(r) => r,
        Err(e) => {
            log::debug!("Counting {table} in {file} failed (treated as 0): {e}");
            return Ok(0);
        }
    };
    match rows.next().await {
        Ok(Some(row)) => Ok(row
            .get_value(0)
            .ok()
            .and_then(|v| v.as_integer().copied())
            .unwrap_or(0) as usize),
        _ => Ok(0),
    }
}

/// 从一个库里把表名列出来（表名就是机器人 id / `{robot_id}_vec`）。
///
/// 只有命令行报告（下面的 `report_vector_dimensions` 测试）用它来"自动认出有哪些
/// 机器人"，所以挂在 `cfg(test)` 下——生产代码里没有调用点。
#[cfg(test)]
pub(crate) async fn table_names(path: &str) -> Vec<String> {
    let Some(db) = open_read_only(path).await else {
        return Vec::new();
    };
    let Ok(conn) = db.connect() else {
        return Vec::new();
    };
    let Ok(mut rows) = conn
        .query(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
            (),
        )
        .await
    else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    while let Ok(Some(row)) = rows.next().await {
        if let Some(t) = row.get_value(0).ok().and_then(|v| v.as_text().cloned()) {
            names.push(t);
        }
    }
    names
}

/// 猜出有哪些机器人。
///
/// 表名就是机器人 id：主表 `{robot_id}`（QA 数据 / 意图短语），向量表 `{robot_id}_vec`
/// 只在真的写进第一行向量时才懒创建（见 `kb::qa::save` / `kb::doc::save_doc_embedding`），
/// 所以**不能**只靠 `_vec` 认机器人——一只没上传过文档、也没有问答的机器人只有主表。
/// 内部表（`sqlite_*`、turso 的自增序列）要排掉。
///
/// 只用于**诊断展示**，不做存在性校验。
#[cfg(test)]
pub(crate) fn robots_from_table_names(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        if n.starts_with("sqlite_") || n.starts_with("__turso_internal") {
            continue;
        }
        let id = n.strip_suffix("_vec").unwrap_or(n.as_str());
        if !id.is_empty() && !out.iter().any(|x| x == id) {
            out.push(String::from(id));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 维度就是 blob 字节数 / 4。这条关系是这份诊断的全部依据，所以单独钉住：
    /// 1024 维 f32 = 4096 字节；长度不是 4 的倍数时向下取整（坏数据不 panic）。
    #[test]
    fn dimension_is_blob_bytes_over_four() {
        assert_eq!(dims_of_blob(&vec![0u8; 1024 * 4]), 1024);
        assert_eq!(dims_of_blob(&vec![0u8; 3072 * 4]), 3072);
        assert_eq!(dims_of_blob(&[]), 0);
        assert_eq!(dims_of_blob(&[0u8; 5]), 1);
    }

    #[test]
    fn dims_are_deduped_and_sorted() {
        let mut d: Vec<usize> = Vec::new();
        push_dim(&mut d, 1024);
        push_dim(&mut d, 384);
        push_dim(&mut d, 1024);
        assert_eq!(d, vec![384, 1024]);
    }

    /// 报告的两条硬性质：
    ///
    /// 1. 真实的向量表是**懒创建**的，所以"检测"必须能回答"表还不存在"，而不是报错；
    /// 2. `PRAGMA table_info` 里的类型名**不保留维数**（实测：`F32_BLOB(3072)` 建表后
    ///    回的是 `F32_BLOB`），所以真实维度只能从 blob 长度数出来。
    #[tokio::test]
    async fn declared_type_has_no_dimension_and_dims_come_from_the_blob() {
        let dir = std::env::temp_dir().join(format!("dsh-vr-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t.dat");
        let path = path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&path);

        {
            let db = turso::Builder::new_local(&path).build().await.unwrap();
            let conn = db.connect().unwrap();
            // 建表声明 3072 维，实际写 3 个数：两者不一致，用来证明"维度不看声明"。
            conn.execute(
                "CREATE TABLE r1_vec (id INTEGER PRIMARY KEY, qa_vec F32_BLOB(3072) NOT NULL)",
                (),
            )
            .await
            .unwrap();
            conn.execute(
                "INSERT INTO r1_vec (qa_vec) VALUES (vector32('[1,2,3]'))",
                (),
            )
            .await
            .unwrap();
        }

        let db = open_read_only(&path).await.expect("file exists");
        let conn = db.connect().unwrap();
        let t = Target {
            file: "t.dat",
            table: String::from("r1_vec"),
            column: "qa_vec",
        };
        let c = inspect(&conn, &t).await;
        assert!(c.exists);
        assert_eq!(
            c.declared_type, "F32_BLOB",
            "turso 把类型名归一化了：括号里的维数在建表时就丢了"
        );
        assert_eq!(c.rows, 1);
        assert_eq!(c.dims, vec![3], "维度来自 blob 长度，不是类型声明");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 表还没建时不能报错，只能是 `exists = false`：向量表在第一次写入前不存在。
    #[tokio::test]
    async fn a_missing_table_reports_not_created() {
        let dir = std::env::temp_dir().join(format!("dsh-vr-miss-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.dat");
        let path = path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&path);
        {
            let db = turso::Builder::new_local(&path).build().await.unwrap();
            let conn = db.connect().unwrap();
            conn.execute("CREATE TABLE something_else (a INTEGER)", ())
                .await
                .unwrap();
        }
        let db = open_read_only(&path).await.unwrap();
        let conn = db.connect().unwrap();
        let c = inspect(
            &conn,
            &Target {
                file: "empty.dat",
                table: String::from("nope_vec"),
                column: "qa_vec",
            },
        )
        .await;
        assert!(!c.exists);
        assert_eq!(c.rows, 0);
        assert!(c.dims.is_empty());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 报告用：跑一次就能看到 `data/*.dat` 里每个机器人实际存了多少维。
    ///
    /// 机器人 id 自动从表名推出来，也可以显式给：`DSH_ROBOT_IDS=id1,id2`。
    /// 需要 `--nocapture` 才看得到输出；`data/` 不存在（CI、刚 clone）时只打印
    /// 一行说明，不失败。
    #[tokio::test]
    async fn report_vector_dimensions() {
        if !Path::new("./data").exists() {
            println!("没有 ./data 目录：跳过（CI / 新 clone 的正常状态）");
            return;
        }
        let mut names: Vec<String> = Vec::new();
        for f in ["qa.dat", "doc.dat", "phrase.dat"] {
            let path = format!("./data/{f}");
            let t = table_names(&path).await;
            if !t.is_empty() {
                println!("{path} 里的表：{}", t.join(", "));
            }
            names.extend(t);
        }
        let explicit: Vec<String> = std::env::var("DSH_ROBOT_IDS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let robots = if explicit.is_empty() {
            robots_from_table_names(&names)
        } else {
            explicit
        };
        if robots.is_empty() {
            println!("没从表名里认出任何机器人（还没有机器人写过这类数据）");
            return;
        }
        for id in robots {
            println!("== 机器人 {id} ==");
            for c in robot_vector_report(&id).await {
                let state = if !c.exists {
                    String::from("尚未建表（没有向量）")
                } else if c.dims.is_empty() {
                    format!("表在但没有行（建表声明 {}）", c.declared_type)
                } else {
                    let dims = c
                        .dims
                        .iter()
                        .map(|d| d.to_string())
                        .collect::<Vec<_>>()
                        .join(" / ");
                    let warn = if c.dims.len() > 1 {
                        "  <== 混合维度！检索会直接报错"
                    } else {
                        ""
                    };
                    format!("dims={dims}（建表声明 {}）{warn}", c.declared_type)
                };
                println!("  {:<46} rows={:<6} {}", c.source, c.rows, state);
            }
        }
    }
}
