<script setup>
import { ref, reactive, computed, onMounted, onUnmounted, provide } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ElMessage } from "element-plus";
import { copyProperties, httpReq, getRobotType } from "../../assets/tools.js";
import { useI18n } from "vue-i18n";
import chatPicThumbnail from "@/assets/usedByLlmChatNode-thumbnail.png";
import chatPic from "@/assets/usedByLlmChatNode.png";
import sentenceEmbeddingPicThumbnail from "@/assets/usedBySentenceEmbedding-thumbnail.png";
import sentenceEmbeddingPic from "@/assets/usedBySentenceEmbedding.png";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const robotId = route.params.robotId;
const robotType = getRobotType(robotId);
const maxSessionIdleMin = ref(30);

const goBack = () => {
    router.push({ name: "robotDetail", params: { robotId: robotId } });
};

const similarityThreshold = ref(85);
const defaultEmailVerificationRegex =
    "[-\\w\\.\\+]{1,100}@[A-Za-z0-9]{1,30}[A-Za-z\\.]{2,30}";
const settings = reactive({
    version: 1017000,
    maxSessionIdleSec: 1800,
    smtpHost: "",
    smtpUsername: "",
    smtpPassword: "",
    smtpTimeoutSec: 60,
    emailVerificationRegex: "",
    chatProvider: {
        provider: {
            id: "",
            model: "",
        },
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        // max_token_len: 200,
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        maxResponseTokenLength: 200,
        proxyUrl: "",
    },
    sentenceEmbeddingProvider: {
        provider: {
            id: "",
            model: "",
        },
        similarityThreshold: 0.85,
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        // 期望的向量维度。null = 自动（由模型决定），这时后端不会发 dimensions 键。
        // 这是**当前 provider 生效的那一份**，后端只读它。
        dimensions: null,
        // 每个 provider 各自那份维度，{ HuggingFace: 8192, OpenAICompatible: 16 }。
        // 「本地模型 / 在线模型」是两类互不相干的模型，维度跟着模型走，所以必须分开
        // 存：只留一份的话，在本地填 8192、保存、切到在线，在线会显示 8192 —— 一个
        // 用户从没填过的值。后端不解析它，只负责存下来原样返回。
        dimensionsByProvider: {},
        // 库里现有向量是用哪个模型/维度建的（后端打的标）。它只由**写入**更新，
        // 所以"改了模型还没重新索引"能从这里看出来。null = 从没打过标（老库）。
        indexedEmbedding: null,
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        proxyUrl: "",
    },
    asrProvider: {
        enabled: false,
        provider: {
            id: "",
            model: "",
        },
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        proxyUrl: "",
    },
    ttsProvider: {
        enabled: false,
        provider: {
            id: "",
            model: "",
        },
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        proxyUrl: "",
    },
});
const formLabelWidth = "160px";
const loading = ref(false);
const smtpPassed = ref(false);
const smtpFailed = ref(false);
const smtpFailedDetail = ref("");
const showHfIncorrectChatModelTip = ref(false);
const showHfChatModelDownloadProgress = ref(false);
const showHfIncorrectEmbeddingModelTip = ref(false);
const showHfEmbeddingModelDownloadProgress = ref(false);
const originalSentenceEmbeddingModelId = ref("");
const downloadingUrl = ref("");
const downloadingProgress = ref("");
// 「检测模型」按钮的转圈状态。文件级校验要读几十兆的 tokenizer.json，慢，得让
// 用户看见它在跑。
const checkingChatModel = ref(false);
const checkingEmbeddingModel = ref(false);

// 本地模型的实际落盘目录，由后端给出（见 checkHfModelFiles 调的那个接口）。
// 空串 = 当前不是本地模型，或者接口还没回来。
const chatModelLocalPath = ref("");
const sentenceEmbeddingModelLocalPath = ref("");

// 上一次保存（或刚进页面时）**生效**的本地模型名；这一块存的是在线模型时是空串。
// 「重新加载模型」装的是存库的设置，所以要先知道存库的是哪个，才能判断用户是不是
// 改了下拉框而还没保存。
const savedChatModel = ref("");
const savedEmbeddingModel = ref("");
// 存库的向量维度（就是「当前 provider 生效的那一份」`dimensions`）。
// 和上面两个 ref 一样，用来判断用户改了但还没保存——只是这一项改动了不会让
// "重新加载模型"变得可疑，而是会让库里已有的向量失效（维度不一致检索直接失败）。
const savedEmbeddingDimensions = ref(null);
// 后端 provider 的形状是 `{id, model}`：只有 HuggingFace 时 model 才是本地模型名。
const savedLocalModelName = (provider) =>
    provider?.id == "HuggingFace" ? provider.model || "" : "";

// HuggingFace 的「请求地址」不是一个真的地址，而是一句"模型会落到哪儿"的提示。
// 前缀留在前端，路径必须由后端给：真正写文件的是后端，只有它能保证这句话
// 和磁盘上的目录一致（前端硬编码的 ./data/models 曾经就和实际根目录不一致）。
const hfLocalPathHint = "Model will be downloaded locally at ";
// 还没拿到具体模型的路径时显示的兜底文案。权威的根目录在后端
// （HUGGING_FACE_MODEL_ROOT），所以这里只是"模型还没选/接口还没回来"时的样子。
const hfLocalRootFallback = hfLocalPathHint + "./data/models";

// 用 computed 而不是把拼好的字符串写回 settings.*.apiUrl：那个字段会随设置一起
// 存库，它只该是 provider 预设的那句提示（后端读设置时不看 HuggingFace 的
// apiUrl）；带路径的字符串会随选中的模型变，写进去只会留下一堆没用的脏值。
const chatApiUrl = computed({
    get: () =>
        settings.chatProvider.provider.id == "HuggingFace" &&
        chatModelLocalPath.value
            ? hfLocalPathHint + chatModelLocalPath.value
            : settings.chatProvider.apiUrl,
    set: (v) => (settings.chatProvider.apiUrl = v),
});
const sentenceEmbeddingApiUrl = computed({
    get: () =>
        settings.sentenceEmbeddingProvider.provider.id == "HuggingFace" &&
        sentenceEmbeddingModelLocalPath.value
            ? hfLocalPathHint + sentenceEmbeddingModelLocalPath.value
            : settings.sentenceEmbeddingProvider.apiUrl,
    set: (v) => (settings.sentenceEmbeddingProvider.apiUrl = v),
});

// 拉一次设置并铺进内存。抽成函数是因为重建索引跑完之后要**再拉一次**——那时后端
// 已经把"库里那份索引是哪来的"标记改成当前配置，设置页那条警告该消了。
async function loadSettings() {
    const r = await httpReq(
        "GET",
        "management/settings",
        { robotId: robotId },
        null,
        null,
    );
    if (r.status != 200) return;
    {
        // `copyProperties` 跳过值为 null / undefined 的键（见 assets/tools.js），
        // 这对"用响应合并默认值"是对的，但**后端明确为 null 的字段就合并不进来**，
        // 内存里会留着上一轮的旧值。本对象里有三个字段合法地就是 null：
        //   dimensions        没有目标维度（＝自动）
        //   dimensionsByProvider / indexedEmbedding   从没设置过
        // 不先清空的话：后端说"本地那份是 null"，界面上却留着在线那份 16，
        // 保存时的"维度变了"判定就会误报。这几个字段的权威来源只有后端，所以
        // 每次 load 都先归零再合并。
        settings.sentenceEmbeddingProvider.dimensions = null;
        settings.sentenceEmbeddingProvider.dimensionsByProvider = {};
        settings.sentenceEmbeddingProvider.indexedEmbedding = null;
        copyProperties(r.data, settings);
        maxSessionIdleMin.value = settings.maxSessionIdleSec / 60;
        if (settings.sentenceEmbeddingProvider.similarityThreshold != null)
            similarityThreshold.value = Math.round(
                settings.sentenceEmbeddingProvider.similarityThreshold * 100,
            );
        originalSentenceEmbeddingModelId.value =
            r.data.sentenceEmbeddingProvider.provider.id;
        savedChatModel.value = savedLocalModelName(r.data.chatProvider.provider);
        savedEmbeddingModel.value = savedLocalModelName(
            r.data.sentenceEmbeddingProvider.provider,
        );
        savedEmbeddingDimensions.value =
            r.data.sentenceEmbeddingProvider.dimensions ?? null;
        // 这件事必须在 change*Provider 之前做：
        // 把已存地址种进 urlMap。change*Provider 的 else 分支要读这个 map，
        // 不种的话它会拿到 undefined，于是把刚 copyProperties 进来的用户
        // 地址覆盖成预设值——配了远程地址的用户进一次设置页再保存就被改回去了。
        //
        // 这里**不做**空地址兜底：每条记录都是建机器人时由 settings::init() 写下的
        // Settings::default()，那时的 provider 是 HuggingFace、api_url 是空串，
        // 而 HuggingFace 那条分支会把地址显示替换掉。所以"在线模型 + 空地址"
        // 只可能来自用户自己清空——把它悄悄换成 OpenAI 的地址，正是后端刚
        // 去掉的那个行为。空地址会由后端明确报错，这才是我们想要的。
        chatDynamicReqUrlMap.set(
            settings.chatProvider.provider.id,
            settings.chatProvider.apiUrl,
        );
        sentenceEmbeddingDynamicReqUrlMap.set(
            settings.sentenceEmbeddingProvider.provider.id,
            settings.sentenceEmbeddingProvider.apiUrl,
        );
        // 每份 provider 的维度都从库里种进来（后端原样存着那张表），再用当前生效
        // 的 dimensions 覆盖当前 provider 那一项：老记录里没有那张表，这是它唯一
        // 的迁移入口。`??` 而不是 `||` —— 0 和 null 都是要原样保留的值。
        sentenceEmbeddingDimensionsMap.clear();
        for (const [k, v] of Object.entries(
            settings.sentenceEmbeddingProvider.dimensionsByProvider || {},
        ))
            sentenceEmbeddingDimensionsMap.set(k, v ?? null);
        sentenceEmbeddingDimensionsMap.set(
            settings.sentenceEmbeddingProvider.provider.id,
            settings.sentenceEmbeddingProvider.dimensions ?? null,
        );
        restoreSentenceEmbeddingDimensions(
            settings.sentenceEmbeddingProvider.provider.id,
        );
        await changeChatProvider(settings.chatProvider.provider.id);
        await changeSentenceEmbeddingProvider(
            settings.sentenceEmbeddingProvider.provider.id,
        );
    }
    await checkHfModelFiles();
}

onMounted(async () => {
    await loadSettings();
    // 上次打开页面时启动的重建可能还在跑（或者刚跑完/失败了）：只显示，不弹窗。
    await refreshReindexStatus();
    // 上一次保存触发的后台装载可能还在跑，或者上一次装载失败的原因还在：只显示，
    // 不弹窗（那是历史，不是刚发生的事）。
    await refreshModelLoadStatus(false);
});
onUnmounted(() => {
    if (timeoutID != null) clearTimeout(timeoutID);
    if (loadTimeoutID != null) clearTimeout(loadTimeoutID);
    if (reindexTimeoutID != null) clearTimeout(reindexTimeoutID);
});

// 本地模型在候选列表里的 value 就是后端认的模型名（枚举名，如 Qwen3_0_6B）。
// 这里只判断"这个值还在候选里"。老代码还会截掉最后一个空格之后的部分，那是
// 候选项 value 曾经长成"仓库名 (体积)"时留下的残留；现在 value 里没有空格。
const isHfModelName = (options, model) => options.some((o) => o.value == model);

async function checkHfModelFiles() {
    // 每一项：后端认的模型名，以及查完要写回哪两个 ref。
    const items = [];
    const collect = (provider, options, tip, path) => {
        // 不是本地模型：既没有路径可显示，也不必提示下载。
        if (
            provider.id != "HuggingFace" ||
            !isHfModelName(options, provider.model)
        ) {
            tip.value = false;
            path.value = "";
            return;
        }
        items.push({ model: provider.model, tip: tip, path: path });
    };
    collect(
        settings.chatProvider.provider,
        chatModelOptions,
        showHfIncorrectChatModelTip,
        chatModelLocalPath,
    );
    collect(
        settings.sentenceEmbeddingProvider.provider,
        sentenceEmbeddingModelOptions,
        showHfIncorrectEmbeddingModelTip,
        sentenceEmbeddingModelLocalPath,
    );
    if (items.length == 0) return;
    const r = await httpReq(
        "POST",
        "management/settings/model/local/path",
        null,
        null,
        items.map((i) => i.model),
    );
    if (r == null || r.data == null) return;
    for (const i of items) {
        const info = r.data[i.model];
        if (info == null) continue;
        // 路径直接显示给用户，别再自己拼。
        i.path.value = info.path;
        // 只 stat 目录：目录不在 = 还没下载过。这里刻意不走文件级校验——那要读
        // 并解析 tokenizer.json（几兆到几十兆），而这条路径每次打开设置页、每次
        // 保存设置都会走一遍；模型是否真的完整，交给用户主动触发的按钮。
        i.tip.value = info.exists == false;
    }
}

// 「检测模型」：走文件级校验（check/files）——逐个 stat、解析 config.json /
// tokenizer.json、核对 safetensors 体积和 GGUF 魔数。这比上面只 stat 目录贵得多
// （tokenizer.json 几兆到几十兆），所以只在用户点按钮时跑一次，进页面不跑。
// 失败原因（哪个文件不对、为什么）由后端带回，原样弹出：只给一句"校验失败"，
// 用户既不知道该删哪个文件也不知道该重下什么。
async function checkModelFiles(kind, model) {
    const loading =
        kind == "embedding" ? checkingEmbeddingModel : checkingChatModel;
    const tip =
        kind == "embedding"
            ? showHfIncorrectEmbeddingModelTip
            : showHfIncorrectChatModelTip;
    // 校验失败 = 文件真的有问题：弹出原因之外，还要把"缺失/不正确"的提示位打开，
    // 让下载入口和"手动放到 {path}"的说明一起回来，而不是只弹一句错误就没下文。
    // 提示位一开，本按钮按设计就隐藏了（它只在"模型存在"时显示）。
    const fail = (detail) => {
        ElMessage.error(detail || t("botSettings.hfModelCheckFailed"));
        tip.value = true;
    };
    loading.value = true;
    try {
        const r = await httpReq(
            "POST",
            "management/settings/model/check/files",
            null,
            null,
            [model],
        );
        if (r == null) {
            fail();
        } else if (r.data == null) {
            // 接口本身出错（模型名后端不认识会走这里）：原因在 err 里。这不是
            // "文件不对"，所以不动提示位——否则会把一次请求错误说成模型坏了。
            ElMessage.error(
                r.err?.message || t("botSettings.hfModelCheckFailed"),
            );
        } else if (r.data[model] == null) {
            fail();
        } else if (r.data[model].ok) {
            ElMessage.success(t("botSettings.hfModelCheckOk"));
        } else {
            fail(r.data[model].err);
        }
    } finally {
        loading.value = false;
    }
}

// 选中的是不是本地模型（和目录在不在无关）：两个按钮的公共前提。
const isLocalChatModel = computed(
    () =>
        settings.chatProvider.provider.id == "HuggingFace" &&
        isHfModelName(chatModelOptions, settings.chatProvider.provider.model),
);
const isLocalEmbeddingModel = computed(
    () =>
        settings.sentenceEmbeddingProvider.provider.id == "HuggingFace" &&
        isHfModelName(
            sentenceEmbeddingModelOptions,
            settings.sentenceEmbeddingProvider.provider.model,
        ),
);

// 「检测模型」只在**目录已存在**时显示：模型还没下载的时候，那条警告里的下载入口
// 才是用户该点的东西，再摆一个必然失败的检测按钮只会让人困惑。
const canCheckChatModel = computed(
    () => isLocalChatModel.value && !showHfIncorrectChatModelTip.value,
);
const canCheckEmbeddingModel = computed(
    () =>
        isLocalEmbeddingModel.value && !showHfIncorrectEmbeddingModelTip.value,
);

// 下拉框里的选择还没保存。「重新加载模型」装的是**存库**的设置（缓存按 robot_id 存，
// 运行中的对话读的也是存库那份），所以不一致时不能装：否则点了会去装另一个模型，
// 报出来的错和用户屏幕上看到的模型对不上——真实踩过：界面上选的是 Qwen3-1.7B，
// 报的却是 `TinyLlama/...: 系统找不到指定的路径`。
const chatModelNotSaved = computed(
    () => (settings.chatProvider.provider.model || "") != savedChatModel.value,
);
const sentenceEmbeddingModelNotSaved = computed(
    () =>
        (settings.sentenceEmbeddingProvider.provider.model || "") !=
        savedEmbeddingModel.value,
);

// 「库里的向量是用哪个模型/维度建的」。规则必须和后端
// `man::settings::embedding_identity` 一字不差，否则警告会一直误报：
//   v1|<kind>|<模型名>|<维度，0 = 没设>
// 模型名两边的取法都是 `provider.model`：本地候选的 value 是枚举名（如 Qwen3_0_6B），
// 在线候选的 value 就是端点模型名（如 text-embedding-v4）——后端也是这么取的。
const currentEmbeddingIdentity = computed(() => {
    const p = settings.sentenceEmbeddingProvider;
    const kind = p.provider.id == "HuggingFace" ? "huggingface" : "openai-compatible";
    return `v1|${kind}|${p.provider.model || ""}|${p.dimensions || 0}`;
});

// 索引和当前配置对不上就警告。两种情况都**必须**重新索引，但原因不同：
//
// - 换了模型：两个模型的向量空间毫不相干，余弦距离算出来是噪声，而且**不会报错**
//   —— 表现为"回答莫名其妙"，比报错难查得多。
// - 只换了维度：turso 的 `vector_distance_cos` 遇到维度不一致会直接返回错误，
//   于是检索（意图、问答、文档）整体失败。
//
// 只在**打过标**之后才判断：老库没有这个标，报一条讲不清原因的警告只会让人焦虑。
// 另外服务端会在下一次写入时把标改成当前配置——但那时旧向量仍然躺在库里，所以
// 这不是"自动修好了"，真正的修复是重建索引。
const embeddingIndexWarning = computed(() => {
    const indexed = settings.sentenceEmbeddingProvider.indexedEmbedding;
    if (!indexed) return null;
    if (indexed == currentEmbeddingIdentity.value) return null;
    const indexedModel = String(indexed).split("|")[2] || "";
    const currentModel = settings.sentenceEmbeddingProvider.provider.model || "";
    return indexedModel != currentModel
        ? "botSettings.embeddingIndexModelChanged"
        : "botSettings.embeddingIndexDimsChanged";
});

// 保存时换了本地模型，后端会在**后台**装它（不在请求里现装，几十 GB 的权重会把
// 保存接口卡住）。这里轮询状态：装着就显示"正在后台加载模型"，装失败了把后端给的
// 原因（哪个文件不对）显示出来并弹一次。
const chatModelLoad = reactive({ loading: false, model: "", err: "" });
const sentenceEmbeddingModelLoad = reactive({
    loading: false,
    model: "",
    err: "",
});
let loadTimeoutID = null;
// 同一条错误只弹一次：轮询是每秒一次的，每次都弹会把屏幕刷满。
const shownLoadErr = { chat: "", embedding: "" };

// 后端回的是枚举名（如 Qwen3_1_7B），给用户看的是候选项里的 label（仓库 + 体积）；
// 查不到就退回枚举名，至少不是空白。
const hfModelLabel = (options, model) =>
    options.find((o) => o.value == model)?.label || model;

// 取一次后台装载状态；只要还有东西在装，就每秒再取一次（装完自然停）。
//
// `notify` 为 false 时不弹窗，只把状态显示出来：进页面时读到的是**上一次**保存
// 留下的失败，悄悄显示在那一行就够，每次刷新页面都弹一次会很烦。
async function refreshModelLoadStatus(notify = true) {
    if (loadTimeoutID != null) {
        clearTimeout(loadTimeoutID);
        loadTimeoutID = null;
    }
    const r = await httpReq(
        "GET",
        "management/settings/model/load/progress",
        { robotId: robotId },
        null,
        null,
    );
    if (r == null || r.data == null) return;
    for (const [kind, src, dst] of [
        ["chat", r.data.chat, chatModelLoad],
        ["embedding", r.data.embedding, sentenceEmbeddingModelLoad],
    ]) {
        if (src == null) continue;
        dst.loading = src.loading == true;
        dst.model = src.model || "";
        dst.err = src.err || "";
        if (!dst.err) shownLoadErr[kind] = "";
        else if (dst.err != shownLoadErr[kind]) {
            // 原因原样弹出来（哪个文件不对、为什么），不要只说一句"加载失败"。
            // 同一条只弹一次：轮询是每秒一次的。
            shownLoadErr[kind] = dst.err;
            if (notify) ElMessage.error(dst.err);
        }
    }
    if (chatModelLoad.loading || sentenceEmbeddingModelLoad.loading)
        loadTimeoutID = setTimeout(refreshModelLoadStatus, 1000);
}

// 「重新加载模型」：让后端把**当前保存的**本地模型重新装进内存（后台装，进度走上面
// 那套轮询）。用在"文件坏了 → 补好 → 不想等下一次对话、也不想重启"这条路上。
//
// 拼错的 kind / 这块用的是在线模型，都会由后端明确报错——那说明界面状态和后端不一致，
// 应该让用户看见，而不是静默什么都不做。
async function reloadModel(kind) {
    const notSaved =
        kind == "embedding"
            ? sentenceEmbeddingModelNotSaved.value
            : chatModelNotSaved.value;
    if (notSaved) {
        // 选择还没保存。保存本身就会在后台装它，所以这里只提示，不去装存库的那个
        // （那正是"选了 Qwen3 却报 TinyLlama 找不到"的由来）。
        ElMessage.warning(t("botSettings.hfModelReloadSaveFirst"));
        return;
    }
    const r = await httpReq(
        "POST",
        "management/settings/model/load",
        { robotId: robotId, kind: kind },
        null,
        null,
    );
    if (r == null || r.status != 200) {
        ElMessage.error(
            r?.err?.message || t("botSettings.hfModelReloadFailed"),
        );
        return;
    }
    // 后端在返回之前就把状态置成"加载中"了，这里立刻接上进度显示。
    await refreshModelLoadStatus();
}

async function save() {
    // 两种情况都会让库里的旧向量失效，都要先问一句：
    //
    // - 换了 provider（本地模型 ↔ 在线模型）：这是"模型换了"，向量空间整个不同。
    // - 只改了维度（Matryoshka 截断）：同一模型的另一段长度，`vector_distance_cos`
    //   遇到维度不一致会直接报错，检索整体挂掉。
    //
    // 注意这里比的是**当前 provider 那一份**与「存库时那一份」：provider 一起比是
    // 因为同一份 `savedEmbeddingDimensions` 在切 provider 后就变成另一栏的基准了。
    const savedDims = savedEmbeddingDimensions.value;
    const currentDims = settings.sentenceEmbeddingProvider.dimensions ?? null;
    const dimsChanged = (savedDims ?? null) !== currentDims;
    const providerChanged =
        originalSentenceEmbeddingModelId.value !=
        settings.sentenceEmbeddingProvider.provider.id;
    if (providerChanged || dimsChanged) {
        ElMessageBox.confirm(
            t(
                providerChanged
                    ? "botSettings.modelChangedWarning"
                    : "botSettings.embeddingDimensionsChangedWarning",
            ),
            t("common.warning"),
            {
                confirmButtonText: t("common.confirm"),
                cancelButtonText: t("common.cancel"),
                type: "warning",
                dangerouslyUseHTMLString: true,
            },
        )
            .then(() => {
                saveSettings();
            })
            .catch(() => {});
    } else saveSettings();
}

async function saveSettings() {
    if (!settings.emailVerificationRegex)
        settings.emailVerificationRegex = defaultEmailVerificationRegex;
    settings.maxSessionIdleSec = maxSessionIdleMin.value * 60;
    settings.sentenceEmbeddingProvider.similarityThreshold =
        similarityThreshold.value / 100;
    // 把"当前 provider 那一份维度"定稿到 `dimensions`（后端只读它），并把
    // `dimensionsByProvider` 刷成最新。用户可能一次都没动过输入框，所以不能只靠
    // 输入框的 setter。
    syncSentenceEmbeddingDimensions();
    const r = await httpReq(
        "POST",
        "management/settings",
        { robotId: robotId },
        null,
        settings,
    );
    if (r.status == 200) {
        ElMessage({ type: "success", message: t("common.saved") });
        // 现在存库的就是界面上选的这份了：先记下来，再刷新提示（不然刚保存完
        // 「重新加载」还会以为"没保存"）。
        savedChatModel.value = savedLocalModelName(settings.chatProvider.provider);
        savedEmbeddingModel.value = savedLocalModelName(
            settings.sentenceEmbeddingProvider.provider,
        );
        // 基准也要跟着更新，否则"存完再点一次保存"会拿旧基准再弹一次同样的警告。
        // `originalSentenceEmbeddingModelId` 故意**不动**：它记录的是"进这个页面时
        // 存库的是哪个 provider"，是"这次会话里换过 provider"的依据，语义不同。
        savedEmbeddingDimensions.value =
            settings.sentenceEmbeddingProvider.dimensions ?? null;
        await checkHfModelFiles();
        // 换了本地模型的话，后端已经开始在后台装它了：这里开始（或继续）显示进度。
        await refreshModelLoadStatus();
    } else {
        const m = t(r.err.message);
        ElMessage.error(m ? m : r.err.message);
    }
}

let timeoutID = null;

// `m` 是模型名（下载接口的请求体就是它），`kind` 说明这次下载属于哪一块。
// 以前靠 `m == "sentenceEmbedding"` 区分，可两个按钮传的都是模型名
// （如 AllMiniLML6V2），这个判断永远不成立 —— 于是句向量模型下载时亮的是
// 对话那一块的进度条，句向量自己的进度条一个都不显示。归属由调用方明说。
async function downloadModels(m, kind) {
    const r = await httpReq(
        "GET",
        "management/settings/model/download/progress",
        null,
        null,
        null,
    );
    if (r != null && r.data != null && r.data.downloading) {
        const msg =
            "Downloading: " +
            r.data.url +
            " (" +
            ((r.data.downloadedLen / r.data.totalLen) * 100).toFixed(2) +
            "%), please wait until it finish.";
        ElMessage.error(msg);
        return;
    }
    httpReq(
        "POST",
        "management/settings/model/download",
        { robotId: robotId, m: m },
        null,
        m,
    ).then((r) => {
        if (r == null || r.status != 200) {
            ElMessage.error("Download failed: " + r.err.message);
            return;
        }
        if (kind == "embedding") {
            showHfIncorrectEmbeddingModelTip.value = false;
            showHfEmbeddingModelDownloadProgress.value = true;
        } else {
            // 本地对话模型的下载。进度以前是写进"文本生成"那一块的 ref（那个
            // 区块已经并掉了），而对话区块自己绑的 showHfChatModelDownloadProgress
            // 从来没人赋值——于是下载中一个进度条都不显示。现在写它自己这个。
            showHfIncorrectChatModelTip.value = false;
            showHfChatModelDownloadProgress.value = true;
        }
        timeoutID = setTimeout(async () => {
            await showDownloadProgress();
        }, 1000);
    });
}

async function downloadComplete() {
    clearTimeout(timeoutID);
    showHfIncorrectChatModelTip.value = false;
    showHfChatModelDownloadProgress.value = false;
    showHfIncorrectEmbeddingModelTip.value = false;
    showHfEmbeddingModelDownloadProgress.value = false;
    // 文件刚落盘，重新问一遍"会放到哪儿、那个目录在不在"：路径通常没变，但
    // 目录此时才真正存在，靠它判断的「模型缺失」提示该跟着消失。
    await checkHfModelFiles();
}

async function showDownloadProgress() {
    const r = await httpReq(
        "GET",
        "management/settings/model/download/progress",
        null,
        null,
        null,
    );
    if (r != null && r.data != null) {
        if (r.data.err) {
            ElMessage.error(r.data.err);
            clearTimeout(timeoutID);
            showHfChatModelDownloadProgress.value = false;
            showHfEmbeddingModelDownloadProgress.value = false;
            return;
        } else if (r.data.downloading) {
            downloadingUrl.value = r.data.url;
            downloadingProgress.value = (
                (r.data.downloadedLen / r.data.totalLen) *
                100
            ).toFixed(2);
            timeoutID = setTimeout(async () => {
                await showDownloadProgress();
            }, 1000);
        } else await downloadComplete();
    } else {
        await downloadComplete();
    }
}

const smtpTest = async () => {
    loading.value = true;
    const r = await httpReq(
        "POST",
        "management/settings/smtp/test",
        null,
        null,
        settings,
    );
    if (r.status == 200) {
        smtpPassed.value = true;
        smtpFailed.value = false;
    } else {
        const m = t(r.err.message);
        smtpFailedDetail.value = m ? m : r.err.message;
        smtpPassed.value = false;
        smtpFailed.value = true;
    }
    loading.value = false;
};

const ollamaModels = [];
provide("ollamaModels", { ollamaModels });

// OpenAI 兼容端点的厂商预设。
//
// 厂商不做持久化，而是每次从 apiUrl 反查出来。本次改动里 URL 是唯一决定
// 行为的因素，所以反查结果不可能与实际请求不一致；代价是同一网关下的不同
// 厂商会被归成"自定义"，可以接受——预设的职责只是填 URL 和给模型候选。
//
// apiUrl / embedUrl 必须是**完整的端点路径**（形如 .../v1/chat/completions），
// 不是 base URL：后端就是拿它直接 POST 的。厂商模型名会变，用之前对一下。
const compatibleVendors = [
    {
        key: "openai",
        nameKey: "botSettings.vendorOpenAi",
        chatUrl: "https://api.openai.com/v1/chat/completions",
        chatModels: [
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4-vision-preview",
            "gpt-3.5-turbo",
        ],
        embedUrl: "https://api.openai.com/v1/embeddings",
        // ada-002 已从候选里去掉：2026 年了，它比 3-small 又贵又差。
        // 老设置里存着它的仍然能跑（OpenAI 没下线），只是新配置不再推荐。
        embedModels: ["text-embedding-3-large", "text-embedding-3-small"],
    },
    {
        key: "deepseek",
        nameKey: "botSettings.vendorDeepSeek",
        chatUrl: "https://api.deepseek.com/v1/chat/completions",
        chatModels: ["deepseek-chat", "deepseek-reasoner"],
    },
    {
        key: "zhipu",
        nameKey: "botSettings.vendorZhipu",
        chatUrl: "https://open.bigmodel.cn/api/paas/v4/chat/completions",
        chatModels: ["glm-4-plus", "glm-4-air", "glm-4-flash"],
        embedUrl: "https://open.bigmodel.cn/api/paas/v4/embeddings",
        embedModels: ["embedding-3", "embedding-2"],
    },
    {
        key: "qwen",
        nameKey: "botSettings.vendorQwen",
        chatUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
        chatModels: ["qwen-max", "qwen-plus", "qwen-turbo"],
        embedUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1/embeddings",
        // qwen3.7-text-embedding 是通义最新的向量模型（默认 1024 维，可选到 2560，
        // 单行 128K tokens）；v4 就是 Qwen3 训练的向量模型，支持 64~2048 维、
        // 上下文 8K；v3 / v2 留着是因为老设置里可能存着它们。
        embedModels: [
            "qwen3.7-text-embedding",
            "text-embedding-v4",
            "text-embedding-v3",
            "text-embedding-v2",
        ],
    },
    {
        key: "moonshot",
        nameKey: "botSettings.vendorMoonshot",
        chatUrl: "https://api.moonshot.cn/v1/chat/completions",
        chatModels: ["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"],
    },
    // MiniMax 的 OpenAI 兼容端点在 api.minimax.io（国际版）/ api.minimaxi.com
    // （中国大陆版，注意域名末尾多一个 i），两者按账号区域划分，密钥不通用。
    // 这里填国际版；国内账号把域名换成 api.minimaxi.com 即可，路径不变。
    //
    // 只有对话端点，**没有 embeddings**：MiniMax 没有 OpenAI 兼容的向量接口
    // （它的 embedding 走原生 /v1/embeddings，请求体是 texts 而不是 input）。
    // 于是这里不写 embedUrl，向量区块的厂商列表里自然就不会出现 MiniMax，
    // 与 moonshot / groq / openrouter 一致。
    {
        key: "minimax",
        nameKey: "botSettings.vendorMinimax",
        chatUrl: "https://api.minimax.io/v1/chat/completions",
        chatModels: [
            "MiniMax-M3",
            "MiniMax-M2.7",
            "MiniMax-M2.7-highspeed",
            "MiniMax-M2.5",
            "MiniMax-M2.5-highspeed",
            "MiniMax-M2.1",
            "MiniMax-M2.1-highspeed",
            "MiniMax-M2",
        ],
    },
    {
        key: "siliconflow",
        nameKey: "botSettings.vendorSiliconFlow",
        chatUrl: "https://api.siliconflow.cn/v1/chat/completions",
        chatModels: ["deepseek-ai/DeepSeek-V3", "Qwen/Qwen2.5-7B-Instruct"],
        embedUrl: "https://api.siliconflow.cn/v1/embeddings",
        // Qwen3-Embedding 系列是硅基流动 2025 年上线的，MTEB 上明显强过 bge-m3；
        // 模型 id 必须带 `Qwen/` 前缀（docs.siliconflow.com 的示例就是这个写法）。
        embedModels: [
            "Qwen/Qwen3-Embedding-8B",
            "Qwen/Qwen3-Embedding-4B",
            "Qwen/Qwen3-Embedding-0.6B",
            "BAAI/bge-m3",
            "BAAI/bge-large-zh-v1.5",
        ],
    },
    {
        key: "groq",
        nameKey: "botSettings.vendorGroq",
        chatUrl: "https://api.groq.com/openai/v1/chat/completions",
        chatModels: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
    },
    {
        key: "openrouter",
        nameKey: "botSettings.vendorOpenRouter",
        chatUrl: "https://openrouter.ai/api/v1/chat/completions",
        chatModels: ["openai/gpt-4o-mini", "anthropic/claude-3.5-sonnet"],
    },
    // Ollama 从 v0.1.24 起提供 OpenAI 兼容端点，官方文档明确支持流式、`max_tokens`
    // 和 base64 图片（`image_url` 的字符串和对象两种写法都收），所以我们走统一的
    // `/v1/chat/completions`，不再单开一条原生 `/api/chat` 的路径。
    // 向量同理走 `/v1/embeddings`。
    {
        key: "ollama",
        nameKey: "botSettings.vendorOllama",
        chatUrl: "http://localhost:11434/v1/chat/completions",
        chatModels: [],
        embedUrl: "http://localhost:11434/v1/embeddings",
        // 都是 Ollama 官方库里 `c=embedding` 那一档的模型名（拉取用的名字，
        // 不带 tag 的用默认 tag）。qwen3-embedding 只有 0.6b/4b/8b 三个档。
        embedModels: [
            "qwen3-embedding:8b",
            "embeddinggemma",
            "nomic-embed-text-v2-moe",
            "nomic-embed-text",
            "bge-m3",
            "mxbai-embed-large",
        ],
    },
    {
        key: "vllm",
        nameKey: "botSettings.vendorVllm",
        chatUrl: "http://localhost:8000/v1/chat/completions",
        chatModels: [],
        embedUrl: "http://localhost:8000/v1/embeddings",
        embedModels: [],
    },
    {
        key: "lmstudio",
        nameKey: "botSettings.vendorLmStudio",
        chatUrl: "http://localhost:1234/v1/chat/completions",
        chatModels: [],
        embedUrl: "http://localhost:1234/v1/embeddings",
        embedModels: [],
    },
    // 留空的地址，让用户自己填。
    {
        key: "custom",
        nameKey: "botSettings.vendorCustom",
        chatUrl: "",
        chatModels: [],
        embedUrl: "",
        embedModels: [],
    },
];
const customVendor = compatibleVendors[compatibleVendors.length - 1];

// 只归一化尾斜杠和大小写，**不做前缀匹配**：用户通过网关/代理走的路径是
// 合法的，前缀匹配会把用户自己填的地址重新贴成别家的标签。
const normalizeUrl = (u) => (u || "").trim().replace(/\/+$/, "").toLowerCase();
const vendorUrlKey = (kind) => (kind == "embedding" ? "embedUrl" : "chatUrl");
const vendorUrl = (v, kind) => v[vendorUrlKey(kind)] || "";
// 走这个而不是直接写 v.chatModels / v.embedModels：向量区块复制对话区块的
// 代码时，最后一个 chatModels 忘了改是很容易发生的事。
const vendorModels = (v, kind) =>
    (kind == "embedding" ? v.embedModels : v.chatModels) || [];
// 某区块可选的厂商。没有 embeddings 端点的（moonshot / groq / openrouter）
// 不出现在向量区块的列表里。
const vendorsFor = (kind) =>
    compatibleVendors.filter((v) => v.key == "custom" || vendorUrl(v, kind));
const vendorByKey = (key) =>
    compatibleVendors.find((v) => v.key == key) || customVendor;
// 从地址反查厂商；认不出来归到"自定义"。空地址返回空串，交给调用方兜底。
const deriveVendorKey = (apiUrl, kind) => {
    const u = normalizeUrl(apiUrl);
    if (!u) return "";
    const k = vendorUrlKey(kind);
    const hit = compatibleVendors.find((v) => v[k] && normalizeUrl(v[k]) == u);
    return hit ? hit.key : "custom";
};
// 选中「自定义」时调用：把地址框里的**厂商预设地址**清掉。
//
// 为什么必须清：下拉的值不是用户状态，而是每次从地址反查出来的（deriveVendorKey）。
// 不清的话 refresh*Vendor 会拿框里那个旧厂商的地址回查，把下拉改回旧厂商——表现就是
// "选自定义完全没反应"（「自定义」没有预设地址，else 分支什么都不写，于是反查结果必是
// 旧厂商）。清掉之后反查得到空串，归属自然是「自定义」，下拉显示、模型候选、保存后
// 重新加载三者才一致。
//
// 为什么可以清：onMounted 在 change*Provider 之前就把已存地址种进了 map，空地址在
// map 里是 "" 而不是 undefined，重载时走 `u != null` 分支原样保留，不会被
// OpenAICompatible 的预设地址覆盖回来（详见 onMounted 里那段注释）。
//
// 为什么只清预设：反查为空串说明本来就是「自定义」，没什么可清；反查为 custom 说明
// 那是用户自己手输的地址，绝不能动。只有"再点一下另一边就能选回来的厂商预设"才清。
const clearPresetApiUrl = (provider, kind) => {
    const derived = deriveVendorKey(provider.apiUrl, kind);
    if (derived && derived != "custom") provider.apiUrl = "";
};
// 值非空且不在候选里就补一个选项。**必要**，不是可选：Element Plus 关闭态
// 显示的是匹配到的 option 的 label，缺了它，重载后的自定义模型名会显示成
// 一个空白框（值其实是对的）。
//
// 代价：切换厂商后，上一个厂商的模型名会作为候选留在列表里（因为它是当前
// 选中值）。没法区分"用户手输的名字"和"上个厂商的预设名"——两者在存储里
// 都是同一个字符串。留着比丢掉好：丢掉会让当前选择显示成空白框。真选了
// 不对的模型，后端会带着端点地址报错。
const ensureOption = (list, value) => {
    if (!value) return;
    if (list.find((d) => d.value == value) == null) list.unshift({ label: value, value });
};

// https://docs.spring.io/spring-ai/reference/api/chat/completions.html
const chatProviders = [
    {
        id: "HuggingFace",
        nameKey: "botSettings.providerHuggingFace",
        apiUrl: hfLocalRootFallback,
        apiUrlDisabled: true,
        showApiKeyInput: false,
        models: [
            {
                label: "microsoft/Phi-3-mini-4k-instruct (7.7GB)",
                value: "Phi3Mini4kInstruct",
            },
            // Phi-4-mini（V4Mini）。架构与 Phi-3 同一个（candle 里共用 phi3），
            // 权重是 BF16 的两片分片，所以体积与 Phi-3-mini 相当。
            {
                label: "microsoft/Phi-4-mini-instruct (7.7GB)",
                value: "Phi4MiniInstruct",
            },
            {
                label: "google/gemma-2b-it (4.9GB)",
                value: "Gemma2bInstruct",
                need_auth_header: true,
            },
            {
                label: "google/gemma-7b-it (12.1GB)",
                value: "Gemma7bInstruct",
                need_auth_header: true,
            },
            // 本地 Gemma 4（多模态：文本 + 图像）。Apache-2.0、免访问令牌。
            // 体积是单文件 BF16 safetensors 的实际大小（"E" 是 effective
            // parameters，E2B/E4B 的权重其实有 51 亿 / 80 亿参数）。
            {
                label: "google/gemma-4-E2B-it (10.2GB, text+vision)",
                value: "Gemma4E2BIt",
            },
            {
                label: "google/gemma-4-E4B-it (16.0GB, text+vision)",
                value: "Gemma4E4BIt",
            },
            {
                label: "google/gemma-4-12B-it (24.0GB, text+vision)",
                value: "Gemma412BIt",
            },
            {
                label: "TinyLlama/TinyLlama-1.1B-Chat-v1.0 (2.2GB)",
                value: "TinyLlama1_1bChatV1_0",
            },
            // 本地千问（Qwen3）。权重是 GGUF 量化版（Q4_K_M），分词器另外从官方
            // base 仓库下载 —— GGUF 仓库本身不带 tokenizer.json。体积是
            // Q4_K_M 实际大小。
            {
                label: "Qwen/Qwen3-0.6B (GGUF Q4_K_M, 397MB)",
                value: "Qwen3_0_6B",
            },
            {
                label: "Qwen/Qwen3-1.7B (GGUF Q4_K_M, 1.1GB)",
                value: "Qwen3_1_7B",
            },
            {
                label: "Qwen/Qwen3-4B (GGUF Q4_K_M, 2.5GB)",
                value: "Qwen3_4B",
            },
            {
                label: "Qwen/Qwen3-8B (GGUF Q4_K_M, 5.0GB)",
                value: "Qwen3_8B",
            },
            {
                label: "Qwen/Qwen3-14B (GGUF Q4_K_M, 9.0GB)",
                value: "Qwen3_14B",
            },
            {
                label: "Qwen/Qwen3-32B (GGUF Q4_K_M, 19.8GB)",
                value: "Qwen3_32B",
            },
            {
                label: "Qwen/Qwen3-30B-A3B-Instruct-2507 (GGUF Q4_K_M, 17.3GB)",
                value: "Qwen3_30B_A3B_Instruct_2507",
            },
        ],
    },
    {
        id: "OpenAICompatible",
        nameKey: "botSettings.providerOpenAiCompatible",
        // 地址只是没配过时的兜底，真正的值来自 onMounted 里种进 map 的已存
        // 记录；changeChatProvider 会拿厂商预设覆盖模型候选。
        apiUrl: "https://api.openai.com/v1/chat/completions",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        models: [],
    },
];

const chatProviderProxyEnabled = ref(false);
const isAddingAnotherChatModel = ref(false);
const anotherChatModel = ref("");
const chatModelSelector = ref();
// 「添加自定义模型名」。原来这里硬编码 chatProviders[2]（数组一旦少一项就
// 静默加错地方），还无条件把 provider.id 改成 "Ollama"。现在加到用户当前
// 选中的那一项，且不再改 provider——那是单选按钮的职责。
const addAnotherChatModel = (m) => {
    if (!m || choosedChatProvider.value == "HuggingFace") return;
    const p = chatProviders.find((d) => d.id == choosedChatProvider.value);
    if (p == null) return;
    ensureOption(chatModelOptions, m);
    ensureOption(p.models, m);
    chatModelSelector.value?.blur();
    settings.chatProvider.provider.model = m;
    anotherChatModel.value = "";
};

// https://docs.spring.io/spring-ai/reference/api/embeddings.html
const sentenceEmbeddingProviders = [
    {
        id: "HuggingFace",
        nameKey: "botSettings.providerHuggingFace",
        apiUrl: hfLocalRootFallback,
        apiUrlDisabled: true,
        showApiKeyInput: false,
        models: [
            {
                label: "sentence-transformers/all-MiniLM-L6-v2 (91MB)",
                value: "AllMiniLML6V2",
            },
            {
                label: "sentence-transformers/paraphrase-MiniLM-L12-v2 (135MB)",
                value: "ParaphraseMLMiniLML12V2",
            },
            {
                label: "sentence-transformers/paraphrase-multilingual-mpnet-base-v2 (1.11GB)",
                value: "ParaphraseMLMpnetBaseV2",
            },
            {
                label: "BAAI/bge-small-en-v1.5 (135MB)",
                value: "BgeSmallEnV1_5",
            },
            { label: "BAAI/bge-base-en-v1.5 (439MB)", value: "BgeBaseEnV1_5" },
            {
                label: "BAAI/bge-large-en-v1.5 (1.35GB)",
                value: "BgeLargeEnV1_5",
            },
            { label: "BAAI/bge-m3 (2.27GB)", value: "BgeM3" },
            {
                label: "nomic-ai/nomic-embed-text-v1.5 (550MB)",
                value: "NomicEmbedTextV1_5",
            },
            {
                label: "intfloat/multilingual-e5-small (472MB)",
                value: "MultilingualE5Small",
            },
            {
                label: "intfloat/multilingual-e5-base (1.11GB)",
                value: "MultilingualE5Base",
            },
            {
                label: "intfloat/multilingual-e5-large (2.24GB)",
                value: "MultilingualE5Large",
            },
            {
                label: "mixedbread-ai/mxbai-embed-large-v1 (1.34GB)",
                value: "MxbaiEmbedLargeV1",
            },
        ],
    },
    {
        id: "OpenAICompatible",
        nameKey: "botSettings.providerOpenAiCompatible",
        apiUrl: "https://api.openai.com/v1/embeddings",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        models: [],
    },
];
const sentenceEmbeddingProviderProxyEnabled = ref(false);
const chatModelOptions = reactive([]);
const chatDynamicReqUrlMap = new Map();
const choosedChatProvider = ref("");
const chatVendorKey = ref("");
// 从当前地址反查厂商写回下拉，并用该厂商的模型候选重建候选列表。
// 两个触发点：选中「在线模型」，以及用户手改地址后失焦（@change，不是逐键）。
const refreshChatVendor = () => {
    const p = chatProviders.find((d) => d.id == "OpenAICompatible");
    const v = vendorByKey(deriveVendorKey(settings.chatProvider.apiUrl, "chat"));
    chatVendorKey.value = v.key;
    p.models.splice(
        0,
        p.models.length,
        ...vendorModels(v, "chat").map((m) => ({ label: m, value: m })),
    );
    ensureOption(p.models, settings.chatProvider.provider.model);
    chatModelOptions.splice(0, chatModelOptions.length, ...p.models);
};
// 选厂商 = 填它的地址（没有预设的「自定义」则清掉预设地址），然后照常刷新。
const applyChatVendorPreset = (key) => {
    const u = vendorUrl(vendorByKey(key), "chat");
    if (u) settings.chatProvider.apiUrl = u;
    else if (key == "custom") clearPresetApiUrl(settings.chatProvider, "chat");
    refreshChatVendor();
};
const changeChatProvider = async (n) => {
    if (choosedChatProvider.value)
        chatDynamicReqUrlMap.set(
            choosedChatProvider.value,
            settings.chatProvider.apiUrl,
        );
    for (let i = 0; i < chatProviders.length; i++) {
        if (chatProviders[i].id == n) {
            if (chatProviders[i].apiUrlDisabled)
                settings.chatProvider.apiUrl = chatProviders[i].apiUrl;
            else {
                // 判断"在不在 map 里"，不是"值真不真"：用户把地址清空后保存，
                // 存下来的就是空串，那是他的选择，不能拿预设把它填回去。
                // map 里没有这一项才说明这个 provider 是第一次被选中。
                const u = chatDynamicReqUrlMap.get(n);
                settings.chatProvider.apiUrl =
                    u != null ? u : chatProviders[i].apiUrl;
            }
            settings.chatProvider.apiUrlDisabled =
                chatProviders[i].apiUrlDisabled;
            settings.chatProvider.showApiKeyInput =
                chatProviders[i].showApiKeyInput;
            choosedChatProvider.value = n;
            // 关掉可能开着的「添加自定义模型名」表单：它的输入框不跟着 provider
            // 变，留着会让用户在切到本地模型后提交一个 HuggingFace 不认的模型名。
            isAddingAnotherChatModel.value = false;
            // 不在这里拉模型列表：加载时也会走到这里，那等于每次进设置页都
            // 打向（可能是内网、可能连不通的）用户端点。拉取只在按钮后面。
            if (n == "OpenAICompatible") refreshChatVendor();
            else
                chatModelOptions.splice(
                    0,
                    chatModelOptions.length,
                    ...chatProviders[i].models,
                );
            break;
        }
    }
    // 存量记录里可能存着已从选择器移除的 provider（本轮的 Ollama）。这里刻意
    // **不动记录本身**——后端仍认得它、原生路径照走，直接保存也不会丢——只是
    // 单选按钮没法高亮它。至少把已存的模型名显示出来，别是一片空白。
    if (!choosedChatProvider.value)
        ensureOption(chatModelOptions, settings.chatProvider.provider.model);
};
const chatModelListLoading = ref(false);
const fetchChatModelList = async () => {
    if (!settings.chatProvider.apiUrl) return;
    chatModelListLoading.value = true;
    try {
        const r = await httpReq(
            "GET",
            "management/settings/model/openai/list",
            {
                url: settings.chatProvider.apiUrl,
                apiKey: settings.chatProvider.apiKey,
                proxyUrl: settings.chatProvider.proxyUrl,
                connectTimeoutMillis: settings.chatProvider.connectTimeoutMillis,
                readTimeoutMillis: settings.chatProvider.readTimeoutMillis,
            },
            null,
            null,
        );
        if (r.status != 200 || !Array.isArray(r.data))
            throw new Error(r.err?.message || "bad response");
        // 端点返回的就是权威列表，直接替换掉厂商预设的那批候选。
        const p = chatProviders.find((d) => d.id == "OpenAICompatible");
        p.models.splice(
            0,
            p.models.length,
            ...r.data.map((m) => ({ label: m, value: m })),
        );
        ensureOption(p.models, settings.chatProvider.provider.model);
        chatModelOptions.splice(0, chatModelOptions.length, ...p.models);
        ElMessage.success(
            t("botSettings.fetchModelListOk", { count: r.data.length }),
        );
    } catch {
        console.error("拉取模型列表失败", r.err?.message || "bad response");
        // 只提示失败，**不动**用户已经手输的模型名：拉取不到的端点照样能用。
        ElMessage.error(t("botSettings.fetchModelListFailed"));
    } finally {
        chatModelListLoading.value = false;
    }
};

const sentenceEmbeddingModelOptions = reactive([]);
const sentenceEmbeddingDynamicReqUrlMap = new Map();
// 每个 provider（本地/在线）各记一份用户填的「向量维度」。理由和上面那张 URL map
// 完全一样：这一栏的当前值只是**当前 provider 的**值，切走再切回来要有地方取回。
//
// 和 URL map 的唯一区别是它**要持久化**：URL map 里那份东西本来就在设置里
// （apiUrl），而维度如果只留一份在设置里，两个 provider 就会互相覆盖
// （在本地填 8192 保存，切到在线也显示 8192）。所以这里多发一张
// `dimensionsByProvider` 表给后端存着，同时把当前 provider 的值同步进
// `dimensions` —— 后端只读后者。
const sentenceEmbeddingDimensionsMap = new Map();

// 把 `dimensions` 同步回 map，并原样维护要发给后端的那张表。
// 三个调用点：切 provider、用户改动维度、保存之前。
const syncSentenceEmbeddingDimensions = () => {
    const p = settings.sentenceEmbeddingProvider;
    const id = p.provider.id;
    if (id) sentenceEmbeddingDimensionsMap.set(id, p.dimensions ?? null);
    const table = {};
    for (const [k, v] of sentenceEmbeddingDimensionsMap) table[k] = v;
    p.dimensionsByProvider = table;
};

// 切到 provider `id` 时取回它那份维度。
//
// 有记录就原样取回（`null` = 自动，也是一个有意义的值，不能和"没记录过"混为一谈）；
// 没记录（第一次见到这个 provider）就用**存库的那一份**兜底一次：老记录里只有
// `dimensions`、没有那张表，这是它唯一的迁移入口。
const restoreSentenceEmbeddingDimensions = (id) => {
    const p = settings.sentenceEmbeddingProvider;
    if (sentenceEmbeddingDimensionsMap.has(id))
        p.dimensions = sentenceEmbeddingDimensionsMap.get(id) ?? null;
    else if (p.provider.id == id) p.dimensions = p.dimensions ?? null;
    else p.dimensions = null;
};

const choosedSentenceEmbeddingProvider = ref("");
// 输入框绑这个，而不是直接绑 `dimensions`：setter 里顺手把当前 provider 那一份
// 记进 map、并刷新要发给后端的那张表。用户每改一次维度就同步一次，所以即使直接
// 点保存（不切 provider）也不会漏。
const embeddingDimensionsInput = computed({
    get: () => settings.sentenceEmbeddingProvider.dimensions,
    set: (v) => {
        settings.sentenceEmbeddingProvider.dimensions = v ?? null;
        syncSentenceEmbeddingDimensions();
    },
});
const sentenceEmbeddingVendorKey = ref("");
const refreshSentenceEmbeddingVendor = () => {
    const p = sentenceEmbeddingProviders.find((d) => d.id == "OpenAICompatible");
    const v = vendorByKey(
        deriveVendorKey(settings.sentenceEmbeddingProvider.apiUrl, "embedding"),
    );
    sentenceEmbeddingVendorKey.value = v.key;
    p.models.splice(
        0,
        p.models.length,
        ...vendorModels(v, "embedding").map((m) => ({ label: m, value: m })),
    );
    ensureOption(p.models, settings.sentenceEmbeddingProvider.provider.model);
    sentenceEmbeddingModelOptions.splice(
        0,
        sentenceEmbeddingModelOptions.length,
        ...p.models,
    );
};
const applySentenceEmbeddingVendorPreset = (key) => {
    // 同 chat：没有预设的「自定义」清掉厂商预设地址，理由和边界见 clearPresetApiUrl。
    const u = vendorUrl(vendorByKey(key), "embedding");
    if (u) settings.sentenceEmbeddingProvider.apiUrl = u;
    else if (key == "custom")
        clearPresetApiUrl(settings.sentenceEmbeddingProvider, "embedding");
    refreshSentenceEmbeddingVendor();
};
const changeSentenceEmbeddingProvider = async (n) => {
    // 维度跟的是**模型**，而「本地模型 / 在线模型」是两类互不相干的模型，所以切
    // 类别时要换一份维度值。但**不能清掉**：那是用户填过的内容，切回来必须还在。
    //
    // 存的是"切走前那个 provider"那一份，所以先用 `choosed*`（它此刻还指向旧
    // provider），不能用 `provider.id`——那是控件当前绑定的值。初始化时
    // `choosed*` 还是空串，整段跳过；`onMounted` 已经替我们把两份都种好了。
    if (choosedSentenceEmbeddingProvider.value) {
        sentenceEmbeddingDimensionsMap.set(
            choosedSentenceEmbeddingProvider.value,
            settings.sentenceEmbeddingProvider.dimensions ?? null,
        );
        sentenceEmbeddingDynamicReqUrlMap.set(
            choosedSentenceEmbeddingProvider.value,
            settings.sentenceEmbeddingProvider.apiUrl,
        );
    }
    for (let i = 0; i < sentenceEmbeddingProviders.length; i++) {
        if (sentenceEmbeddingProviders[i].id == n) {
            if (sentenceEmbeddingProviders[i].apiUrlDisabled)
                settings.sentenceEmbeddingProvider.apiUrl =
                    sentenceEmbeddingProviders[i].apiUrl;
            else {
                // 同 chat：看"在不在 map 里"，不看值真不真。
                const u = sentenceEmbeddingDynamicReqUrlMap.get(n);
                settings.sentenceEmbeddingProvider.apiUrl =
                    u != null ? u : sentenceEmbeddingProviders[i].apiUrl;
            }
            settings.sentenceEmbeddingProvider.apiUrlDisabled =
                sentenceEmbeddingProviders[i].apiUrlDisabled;
            settings.sentenceEmbeddingProvider.showApiKeyInput =
                sentenceEmbeddingProviders[i].showApiKeyInput;
            // 换到另一类别之前，先把它那份维度取回来。
            restoreSentenceEmbeddingDimensions(n);
            choosedSentenceEmbeddingProvider.value = n;
            // 同 chat：关掉可能开着的添加表单。
            isAddingAnotherSentenceEmbeddingModel.value = false;
            // 同 chat：不在加载路径上打用户的端点，拉取只在按钮后面。
            if (n == "OpenAICompatible") refreshSentenceEmbeddingVendor();
            else
                sentenceEmbeddingModelOptions.splice(
                    0,
                    sentenceEmbeddingModelOptions.length,
                    ...sentenceEmbeddingProviders[i].models,
                );
            break;
        }
    }
    // 同 chat：存量的、已从选择器移除的 provider 只保证模型名可见。
    if (!choosedSentenceEmbeddingProvider.value)
        ensureOption(
            sentenceEmbeddingModelOptions,
            settings.sentenceEmbeddingProvider.provider.model,
        );
};
const sentenceEmbeddingModelListLoading = ref(false);

// 「检测已存向量维度」：设置里那一栏 `dimensions` 说的是"**以后**按几维算"，而库里
// 已有的向量是既成事实——改过设置又还没重新索引时两者就不一致，而 turso 的
// `vector_distance_cos` 遇到维度不一致会让**整条检索报错**。所以这个按钮只读地
// 数一遍 blob（维度 = 字节数 / 4），给一个"不看设置、只看数据"的答案。
const checkingVectorDimensions = ref(false);
const vectorDimensionsVisible = ref(false);
const vectorDimensions = reactive({ configured: null, stored: [] });
const checkVectorDimensions = async () => {
    checkingVectorDimensions.value = true;
    try {
        const r = await httpReq(
            "GET",
            "management/settings/embedding/vector-dimensions",
            { robotId: robotId },
            null,
            null,
        );
        if (r.status != 200 || r.data == null)
            throw new Error(r.err?.message || "bad response");
        vectorDimensions.configured = r.data.configured ?? null;
        vectorDimensions.stored = r.data.stored || [];
        vectorDimensionsVisible.value = true;
    } catch (e) {
        ElMessage.error(e?.message || t("botSettings.vectorDimensionsFailed"));
    } finally {
        checkingVectorDimensions.value = false;
    }
};

const vectorDimensionsVerdict = computed(() => {
    const present = vectorDimensions.stored.filter(
        (s) => s.exists && s.dims.length,
    );
    if (!present.length)
        return {
            type: "info",
            key: "botSettings.vectorDimensionsEmpty",
        };
    // 混合维度最严重：`vector_distance_cos` 遇到长度不一致会让整条查询报错。
    if (present.some((s) => s.dims.length > 1))
        return {
            type: "error",
            key: "botSettings.vectorDimensionsMixed",
        };
    const stored = [...new Set(present.flatMap((s) => s.dims))];
    const configured = vectorDimensions.configured;
    // 只在用户**明确填了**维度时才比对：留空表示"由模型决定"，此时这里的 configured
    // 是 null，无从比较——那种情况下以库里实际存着的值为准。
    if (configured != null && (stored.length > 1 || stored[0] != configured))
        return {
            type: "warning",
            key: "botSettings.vectorDimensionsMismatch",
        };
    return { type: "success", key: "botSettings.vectorDimensionsOk" };
});

// 「重建向量索引」：换模型/换维度之后，库里旧向量已经不可用（换模型＝噪声，换维度＝
// 直接报错），而原文都还在，所以只需要重算。这是一次**后台任务**——一份长文档就是
// 几百个 chunk，同步做会把请求挂死——所以点完立刻返回，然后每秒轮询进度。
const reindexing = ref(false);
const reindexStatus = reactive({
    phase: null,
    intents: { done: 0, total: 0, before: 0, after: 0 },
    qa: { done: 0, total: 0, before: 0, after: 0 },
    docs: { done: 0, total: 0, before: 0, after: 0 },
    err: "",
});
let reindexTimeoutID = null;
const reindexPhases = ["starting", "intents", "qa", "docs"];

// 进度条：某一类的 done/total。total 还没统计出来（0）时返回 0，界面显示"准备中"。
const reindexPercent = (p) =>
    p.total > 0 ? Math.min(100, Math.round((p.done / p.total) * 100)) : 0;

const reindexPhaseText = computed(() => {
    switch (reindexStatus.phase) {
        case "starting":
            return t("botSettings.reindexPhaseStarting");
        case "intents":
            return t("botSettings.reindexPhaseIntents");
        case "qa":
            return t("botSettings.reindexPhaseQa");
        case "docs":
            return t("botSettings.reindexPhaseDocs");
        default:
            return "";
    }
});

const refreshReindexStatus = async () => {
    if (reindexTimeoutID != null) {
        clearTimeout(reindexTimeoutID);
        reindexTimeoutID = null;
    }
    const r = await httpReq(
        "GET",
        "management/settings/embedding/reindex/progress",
        { robotId: robotId },
        null,
        null,
    );
    if (r == null || r.data == null) return;
    for (const k of ["intents", "qa", "docs"]) {
        const src = r.data[k];
        if (src == null) continue;
        Object.assign(reindexStatus[k], src);
    }
    reindexStatus.phase = r.data.phase ?? null;
    reindexStatus.err = r.data.err || "";
    reindexing.value = reindexPhases.includes(reindexStatus.phase);
    if (reindexing.value) reindexTimeoutID = setTimeout(refreshReindexStatus, 1000);
    else if (reindexStatus.phase == "done") {
        // 重建已成功：把上一轮留下的失败原因清掉，否则成功提示旁边会一直挂着旧错误。
        reindexStatus.err = "";
        // 重建完成之后库里那份索引就是当前配置了 → 设置页那条"模型/维度变了"的警告
        // 应该消失。后端已经把标记打成新指纹，这里跟着刷新一次设置。
        await loadSettings();
    }
};

// 重建入口。在线（收费）模型先问一句——一次重建就是"每条数据一次调用"。
const startReindex = async () => {
    const remote =
        settings.sentenceEmbeddingProvider.provider.id != "HuggingFace";
    try {
        await ElMessageBox.confirm(
            remote
                ? t("botSettings.reindexConfirmRemote")
                : t("botSettings.reindexConfirm", { model: "" }),
            t("common.warning"),
            {
                confirmButtonText: t("botSettings.reindexStart"),
                cancelButtonText: t("common.cancel"),
                type: "warning",
                dangerouslyUseHTMLString: true,
            },
        );
    } catch {
        return;
    }
    const r = await httpReq(
        "POST",
        "management/settings/embedding/reindex",
        { robotId: robotId },
        null,
        null,
    );
    if (r?.status != 200) {
        ElMessage.error(r?.err?.message || t("botSettings.reindexStartFailed"));
        return;
    }
    reindexStatus.phase = "starting";
    reindexing.value = true;
    await refreshReindexStatus();
};

const fetchSentenceEmbeddingModelList = async () => {
    if (!settings.sentenceEmbeddingProvider.apiUrl) return;
    sentenceEmbeddingModelListLoading.value = true;
    try {
        const r = await httpReq(
            "GET",
            "management/settings/model/openai/list",
            {
                url: settings.sentenceEmbeddingProvider.apiUrl,
                apiKey: settings.sentenceEmbeddingProvider.apiKey,
                proxyUrl: settings.sentenceEmbeddingProvider.proxyUrl,
                connectTimeoutMillis:
                    settings.sentenceEmbeddingProvider.connectTimeoutMillis,
                readTimeoutMillis:
                    settings.sentenceEmbeddingProvider.readTimeoutMillis,
            },
            null,
            null,
        );
        if (r.status != 200 || !Array.isArray(r.data))
            throw new Error(r.err?.message || "bad response");
        // 端点返回的就是权威列表，直接替换掉厂商预设的那批候选。
        const p = sentenceEmbeddingProviders.find(
            (d) => d.id == "OpenAICompatible",
        );
        p.models.splice(
            0,
            p.models.length,
            ...r.data.map((m) => ({ label: m, value: m })),
        );
        ensureOption(p.models, settings.sentenceEmbeddingProvider.provider.model);
        sentenceEmbeddingModelOptions.splice(
            0,
            sentenceEmbeddingModelOptions.length,
            ...p.models,
        );
        ElMessage.success(
            t("botSettings.fetchModelListOk", { count: r.data.length }),
        );
    } catch {
        ElMessage.error(t("botSettings.fetchModelListFailed"));
    } finally {
        sentenceEmbeddingModelListLoading.value = false;
    }
};

const sentenceEmbeddingModelSelector = ref();
const isAddingAnotherSentenceEmbeddingModel = ref(false);
const anotherSentenceEmbeddingModel = ref("");
const addAnotherSentenceEmbeddingModel = (m) => {
    if (!m || choosedSentenceEmbeddingProvider.value == "HuggingFace") return;
    const p = sentenceEmbeddingProviders.find(
        (d) => d.id == choosedSentenceEmbeddingProvider.value,
    );
    if (p == null) return;
    ensureOption(sentenceEmbeddingModelOptions, m);
    ensureOption(p.models, m);
    sentenceEmbeddingModelSelector.value?.blur();
    settings.sentenceEmbeddingProvider.provider.model = m;
    anotherSentenceEmbeddingModel.value = "";
};

const usedByLlmChatNodeBig = [chatPic];
const usedBySentenceEmbeddingBig = [sentenceEmbeddingPic];
</script>
<template>
    <div class="page-header">
        <h1 class="page-title">{{ $t("settings.title") }}</h1>
        <el-button @click="goBack()">{{ $t("common.back") }}</el-button>
    </div>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">{{ $t("settings.commonSettings") }}</div>
        </template>
        <el-form :model="settings" :label-width="formLabelWidth">
            <el-form-item :label="$t('botSettings.prompt3')">
                <el-input-number
                    v-model="maxSessionIdleMin"
                    :min="2"
                    :max="1440"
                />
                <span class="form-item-suffix">{{
                    $t("botSettings.prompt4")
                }}</span>
            </el-form-item>
        </el-form>
    </el-card>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">
                {{ $t("botSettings.chatModel") }}
                <el-tooltip effect="light" placement="right">
                    <template #content>
                        <span v-html="$t('botSettings.chatModelTip')"></span>
                    </template>
                    <el-button circle>?</el-button>
                </el-tooltip>
            </div>
        </template>
        <el-row>
            <el-col :span="16">
                <el-form
                    :model="settings.chatProvider"
                    :label-width="formLabelWidth"
                >
                    <el-form-item :label="t('botSettings.provider')">
                        <el-radio-group
                            v-model="settings.chatProvider.provider.id"
                            @change="changeChatProvider"
                        >
                            <el-radio-button
                                v-for="item in chatProviders"
                                :id="item.id"
                                :key="item.id"
                                :label="item.id"
                                :value="item.id"
                            >
                                {{ $t(item.nameKey) }}
                            </el-radio-button>
                        </el-radio-group>
                    </el-form-item>
                    <el-form-item
                        v-if="
                            settings.chatProvider.provider.id ==
                            'OpenAICompatible'
                        "
                        :label="t('botSettings.vendor')"
                    >
                        <el-select
                            v-model="chatVendorKey"
                            @change="applyChatVendorPreset"
                        >
                            <el-option
                                v-for="item in vendorsFor('chat')"
                                :key="item.key"
                                :label="$t(item.nameKey)"
                                :value="item.key"
                            />
                        </el-select>
                        <div class="form-item-help">
                            {{ $t("botSettings.compatibleApiHint") }}
                        </div>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.reqAddr')">
                        <el-input
                            v-model="chatApiUrl"
                            :disabled="settings.chatProvider.apiUrlDisabled"
                            @change="refreshChatVendor"
                        />
                    </el-form-item>
                    <el-form-item
                        :label="$t('botSettings.apiKey')"
                        v-show="settings.chatProvider.showApiKeyInput"
                    >
                        <el-input
                            v-model="settings.chatProvider.apiKey"
                            show-password
                        />
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.model')"
                        class="model-row"
                    >
                        <el-select
                            ref="chatModelSelector"
                            v-model="settings.chatProvider.provider.model"
                            :placeholder="$t('botSettings.chooseModel')"
                            :filterable="
                                settings.chatProvider.provider.id !=
                                'HuggingFace'
                            "
                            :allow-create="
                                settings.chatProvider.provider.id !=
                                'HuggingFace'
                            "
                            @change="checkHfModelFiles"
                        >
                            <el-option
                                v-for="item in chatModelOptions"
                                :id="item.value"
                                :key="item.value"
                                :label="item.label"
                                :value="item.value"
                            />
                            <template #footer>
                                <el-button
                                    :disabled="
                                        settings.chatProvider.provider.id ==
                                        'HuggingFace'
                                    "
                                    v-if="!isAddingAnotherChatModel"
                                    text
                                    bg
                                    @click="isAddingAnotherChatModel = true"
                                >
                                    {{ $t("botSettings.anotherModel") }}
                                </el-button>
                                <!-- 表单本身也要挡住 HuggingFace：只禁用按钮
                                     不够，表单一旦开着，切到本地模型后它仍在
                                     渲染，提交进去的模型名会让整个设置保存
                                     被 serde 拒绝。 -->
                                <template
                                    v-else-if="
                                        settings.chatProvider.provider.id !=
                                        'HuggingFace'
                                    "
                                >
                                    <el-input
                                        v-model="anotherChatModel"
                                        :placeholder="$t('botSettings.inputModelName')"
                                        style="margin-bottom: 8px"
                                    />
                                    <el-button
                                        type="primary"
                                        @click="
                                            addAnotherChatModel(anotherChatModel)
                                        "
                                    >
                                        {{ $t("botSettings.confirm") }}
                                    </el-button>
                                    <el-button
                                        @click="isAddingAnotherChatModel = false"
                                        >{{ $t("botSettings.cancelLower") }}</el-button
                                    >
                                </template>
                            </template>
                        </el-select>
                        <el-button
                            class="action-btn"
                            v-if="
                                settings.chatProvider.provider.id ==
                                'OpenAICompatible'
                            "
                            :loading="chatModelListLoading"
                            :disabled="!settings.chatProvider.apiUrl"
                            @click="fetchChatModelList"
                        >
                            {{ $t("botSettings.fetchModelList") }}
                        </el-button>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.maxResTokenLen')">
                        <el-input-number
                            v-model="
                                settings.chatProvider.maxResponseTokenLength
                            "
                            :min="10"
                            :max="100000"
                            :step="5"
                        />
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.connTimeout')"
                        v-show="
                            settings.chatProvider.provider.id != 'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="settings.chatProvider.connectTimeoutMillis"
                            :min="100"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.readTimeout')"
                        v-show="
                            settings.chatProvider.provider.id != 'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="settings.chatProvider.readTimeoutMillis"
                            :min="200"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.proxy')"
                        v-show="
                            settings.chatProvider.provider.id != 'HuggingFace'
                        "
                    >
                        <div class="proxy-row">
                            <el-switch
                                v-model="chatProviderProxyEnabled"
                                :active-text="$t('common.enable')"
                            />
                            <el-input
                                v-model="settings.chatProvider.proxyUrl"
                                placeholder="http://127.0.0.1:9270"
                                :disabled="!chatProviderProxyEnabled"
                            />
                        </div>
                    </el-form-item>
                </el-form>
            </el-col>
            <el-col :span="7" :offset="1">
                <div class="usage-note">
                    {{ $t("botSettings.chatModelUsage") }}
                </div>
                <el-image
                    :src="chatPicThumbnail"
                    :zoom-rate="1.2"
                    :max-scale="7"
                    :min-scale="0.2"
                    :preview-src-list="usedByLlmChatNodeBig"
                    :initial-index="4"
                    fit="cover"
                />
            </el-col>
        </el-row>
        <div v-if="chatModelLoad.loading" class="model-load-status">
            {{
                $t("botSettings.hfModelLoading", {
                    model: hfModelLabel(chatModelOptions, chatModelLoad.model),
                })
            }}
        </div>
        <div v-else-if="chatModelLoad.err" class="model-load-status is-error">
            {{ chatModelLoad.err }}
        </div>
        <div v-if="isLocalChatModel" class="model-check-row">
            <el-button
                class="action-btn"
                v-if="canCheckChatModel"
                size="small"
                :loading="checkingChatModel"
                @click="
                    checkModelFiles(
                        'chat',
                        settings.chatProvider.provider.model,
                    )
                "
            >
                {{ $t("botSettings.hfModelCheck") }}
            </el-button>
            <!-- 手动重装：模型存在与否都能点。文件刚补好（提示位还停在"缺失"）
                 或者想确认一次装载失败的原因时，这是唯一不必等下一次对话的入口。 -->
            <el-button
                class="action-btn"
                size="small"
                :loading="chatModelLoad.loading"
                @click="reloadModel('chat')"
            >
                {{ $t("botSettings.hfModelReload") }}
            </el-button>
        </div>
        <el-alert
            v-if="showHfIncorrectChatModelTip"
            type="warning"
            :closable="false"
            class="hf-alert"
        >
            <template #title>
                {{ $t("botSettings.hfModelMissing") }}
                <el-button
                    type="primary"
                    text
                    @click="
                        downloadModels(
                            settings.chatProvider.provider.model,
                            'chat',
                        )
                    "
                >
                    {{ $t("botSettings.hfModelDownloadLink") }}
                </el-button>
                {{ $t("botSettings.hfModelManual", { path: chatModelLocalPath }) }}
            </template>
        </el-alert>
        <div v-if="showHfChatModelDownloadProgress" class="download-progress">
            <div class="download-progress-url">{{ $t("botSettings.downloading") }}: {{ downloadingUrl }}</div>
            <el-progress
                :percentage="Number(downloadingProgress) || 0"
                :stroke-width="14"
                striped
                striped-flow
            />
        </div>
    </el-card>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">
                {{ $t("botSettings.sentenceEmbedding") }}
                <el-tooltip effect="light" placement="right">
                    <template #content>
                        <span
                            v-html="$t('botSettings.sentenceEmbeddingTip')"
                        ></span>
                    </template>
                    <el-button circle>?</el-button>
                </el-tooltip>
            </div>
        </template>
        <el-row>
            <el-col :span="16">
                <el-form
                    :model="settings.sentenceEmbeddingProvider"
                    :label-width="formLabelWidth"
                >
                    <el-form-item :label="t('botSettings.provider')">
                        <el-radio-group
                            v-model="
                                settings.sentenceEmbeddingProvider.provider.id
                            "
                            @change="changeSentenceEmbeddingProvider"
                        >
                            <el-radio-button
                                v-for="item in sentenceEmbeddingProviders"
                                :id="item.id"
                                :key="item.id"
                                :label="item.id"
                                :value="item.id"
                            >
                                {{ $t(item.nameKey) }}
                            </el-radio-button>
                        </el-radio-group>
                    </el-form-item>
                    <el-form-item
                        v-if="
                            settings.sentenceEmbeddingProvider.provider.id ==
                            'OpenAICompatible'
                        "
                        :label="t('botSettings.vendor')"
                    >
                        <el-select
                            v-model="sentenceEmbeddingVendorKey"
                            @change="applySentenceEmbeddingVendorPreset"
                        >
                            <el-option
                                v-for="item in vendorsFor('embedding')"
                                :key="item.key"
                                :label="$t(item.nameKey)"
                                :value="item.key"
                            />
                        </el-select>
                        <div class="form-item-help">
                            {{ $t("botSettings.embeddingApiHint") }}
                        </div>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.reqAddr')">
                        <el-input
                            v-model="sentenceEmbeddingApiUrl"
                            :disabled="
                                settings.sentenceEmbeddingProvider.apiUrlDisabled
                            "
                            @change="refreshSentenceEmbeddingVendor"
                        />
                    </el-form-item>
                    <el-form-item
                        :label="$t('botSettings.apiKey')"
                        v-show="
                            settings.sentenceEmbeddingProvider.showApiKeyInput
                        "
                    >
                        <el-input
                            v-model="settings.sentenceEmbeddingProvider.apiKey"
                            show-password
                        />
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.model')"
                        class="model-row"
                    >
                        <el-select
                            ref="sentenceEmbeddingModelSelector"
                            v-model="
                                settings.sentenceEmbeddingProvider.provider.model
                            "
                            :placeholder="$t('botSettings.chooseModel')"
                            :filterable="
                                settings.sentenceEmbeddingProvider.provider
                                    .id != 'HuggingFace'
                            "
                            :allow-create="
                                settings.sentenceEmbeddingProvider.provider
                                    .id != 'HuggingFace'
                            "
                            @change="checkHfModelFiles"
                        >
                            <el-option
                                v-for="item in sentenceEmbeddingModelOptions"
                                :id="item.value"
                                :key="item.value"
                                :label="item.label"
                                :value="item.value"
                            />
                            <template #footer>
                                <el-button
                                    :disabled="
                                        settings.sentenceEmbeddingProvider
                                            .provider.id == 'HuggingFace'
                                    "
                                    v-if="
                                        !isAddingAnotherSentenceEmbeddingModel
                                    "
                                    text
                                    bg
                                    @click="
                                        isAddingAnotherSentenceEmbeddingModel = true
                                    "
                                >
                                    {{ $t("botSettings.anotherModel") }}
                                </el-button>
                                <!-- 同 chat：只禁用按钮不够，表单本身也要挡住。 -->
                                <template
                                    v-else-if="
                                        settings.sentenceEmbeddingProvider
                                            .provider.id != 'HuggingFace'
                                    "
                                >
                                    <el-input
                                        v-model="anotherSentenceEmbeddingModel"
                                        :placeholder="$t('botSettings.inputModelName')"
                                        style="margin-bottom: 8px"
                                    />
                                    <el-button
                                        type="primary"
                                        @click="
                                            addAnotherSentenceEmbeddingModel(
                                                anotherSentenceEmbeddingModel,
                                            )
                                        "
                                    >
                                        {{ $t("botSettings.confirm") }}
                                    </el-button>
                                    <el-button
                                        @click="
                                            isAddingAnotherSentenceEmbeddingModel = false
                                        "
                                        >{{ $t("botSettings.cancelLower") }}</el-button
                                    >
                                </template>
                            </template>
                        </el-select>
                        <el-button
                            class="action-btn"
                            v-if="
                                settings.sentenceEmbeddingProvider.provider
                                    .id == 'OpenAICompatible'
                            "
                            :loading="sentenceEmbeddingModelListLoading"
                            :disabled="
                                !settings.sentenceEmbeddingProvider.apiUrl
                            "
                            @click="fetchSentenceEmbeddingModelList"
                        >
                            {{ $t("botSettings.fetchModelList") }}
                        </el-button>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.dimensions')">
                        <div class="threshold-row">
                            <el-input-number
                                v-model="embeddingDimensionsInput"
                                :min="16"
                                :max="8192"
                                :step="1"
                                :controls="false"
                                :placeholder="t('botSettings.dimensionsAuto')"
                                style="width: 120px"
                            />
                            <el-tooltip effect="light" placement="right">
                                <template #content>
                                    {{ $t("botSettings.dimensionsTip") }}
                                </template>
                                <el-button circle>?</el-button>
                            </el-tooltip>
                        </div>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.simThres')">
                        <div class="threshold-row">
                            ≥
                            <el-input-number
                                v-model="similarityThreshold"
                                :min="1"
                                :max="99"
                                :step="1"
                            />
                            %
                            <el-tooltip effect="light" placement="right">
                                <template #content>
                                    {{ $t("botSettings.simThresTip") }}
                                </template>
                                <el-button circle>?</el-button>
                            </el-tooltip>
                        </div>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.connTimeout')"
                        v-show="
                            settings.sentenceEmbeddingProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="
                                settings.sentenceEmbeddingProvider
                                    .connectTimeoutMillis
                            "
                            :min="100"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.readTimeout')"
                        v-show="
                            settings.sentenceEmbeddingProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="
                                settings.sentenceEmbeddingProvider
                                    .readTimeoutMillis
                            "
                            :min="500"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.proxy')"
                        v-show="
                            settings.sentenceEmbeddingProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <div class="proxy-row">
                            <el-switch
                                v-model="sentenceEmbeddingProviderProxyEnabled"
                                :active-text="$t('common.enable')"
                            />
                            <el-input
                                v-model="settings.sentenceEmbeddingProvider.proxyUrl"
                                placeholder="http://127.0.0.1:9270"
                                :disabled="!sentenceEmbeddingProviderProxyEnabled"
                            />
                        </div>
                    </el-form-item>
                </el-form>
            </el-col>
            <el-col :span="7" :offset="1">
                <div class="usage-note">
                    {{ $t("botSettings.sentenceEmbeddingUsage") }}
                </div>
                <el-image
                    :src="sentenceEmbeddingPicThumbnail"
                    :zoom-rate="1.2"
                    :max-scale="7"
                    :min-scale="0.2"
                    :preview-src-list="usedBySentenceEmbeddingBig"
                    :initial-index="4"
                    fit="cover"
                />
            </el-col>
        </el-row>
        <el-alert
            v-if="embeddingIndexWarning"
            type="warning"
            :closable="false"
            class="hf-alert"
        >
            <template #title>
                {{
                    $t(embeddingIndexWarning, {
                        indexed: settings.sentenceEmbeddingProvider.indexedEmbedding,
                    })
                }}
            </template>
        </el-alert>
        <div class="model-check-row">
            <el-button
                class="action-btn"
                size="small"
                :loading="checkingVectorDimensions"
                @click="checkVectorDimensions"
            >
                {{ $t("botSettings.checkVectorDimensions") }}
            </el-button>
            <el-button
                class="action-btn"
                size="small"
                type="warning"
                plain
                :loading="reindexing"
                :disabled="reindexing"
                @click="startReindex"
            >
                {{ $t("botSettings.reindex") }}
            </el-button>
            <el-tooltip effect="light" placement="top">
                <template #content>
                    {{ $t("botSettings.reindexTip") }}
                </template>
                <el-button circle size="small">?</el-button>
            </el-tooltip>
        </div>
        <div v-if="reindexing || reindexStatus.phase == 'done'" class="reindex-panel">
            <div class="reindex-phase">
                {{
                    reindexing
                        ? reindexPhaseText
                        : $t("botSettings.reindexDone")
                }}
            </div>
            <div
                v-for="k in ['intents', 'qa', 'docs']"
                :key="k"
                class="reindex-source"
            >
                <span class="reindex-source-name">
                    {{
                        $t(
                            {
                                intents: "botSettings.reindexPhaseIntents",
                                qa: "botSettings.reindexPhaseQa",
                                docs: "botSettings.reindexPhaseDocs",
                            }[k],
                        )
                    }}
                </span>
                <el-progress
                    :percentage="reindexPercent(reindexStatus[k])"
                    :stroke-width="10"
                    style="flex: 1"
                />
                <span class="reindex-count">
                    {{ reindexStatus[k].done }}/{{ reindexStatus[k].total }}
                    <template v-if="!reindexing">
                        ·
                        {{
                            $t("botSettings.reindexRows", {
                                before: reindexStatus[k].before,
                                after: reindexStatus[k].after,
                            })
                        }}
                    </template>
                </span>
            </div>
        </div>
        <div
            v-if="reindexStatus.phase == 'failed' && reindexStatus.err"
            class="model-load-status is-error"
        >
            {{ $t("botSettings.reindexFailed", { err: reindexStatus.err }) }}
        </div>
        <el-dialog
            v-model="vectorDimensionsVisible"
            :title="t('botSettings.vectorDimensionsTitle')"
            width="640px"
        >
            <div class="vector-dims-hint">
                {{ $t("botSettings.vectorDimensionsHint") }}
            </div>
            <div class="vector-dims-configured">
                {{
                    $t("botSettings.vectorDimensionsConfigured", {
                        value:
                            vectorDimensions.configured ??
                            t("botSettings.dimensionsAuto"),
                    })
                }}
            </div>
            <el-alert
                v-if="vectorDimensionsVerdict"
                :type="vectorDimensionsVerdict.type"
                :closable="false"
                class="hf-alert"
            >
                <template #title>
                    {{ $t(vectorDimensionsVerdict.key) }}
                </template>
            </el-alert>
            <el-table :data="vectorDimensions.stored" size="small">
                <el-table-column
                    prop="source"
                    :label="t('botSettings.vectorDimensionsSource')"
                    min-width="240"
                />
                <el-table-column
                    prop="rows"
                    :label="t('botSettings.vectorDimensionsRows')"
                    width="80"
                />
                <el-table-column
                    :label="t('botSettings.vectorDimensionsDims')"
                    width="120"
                >
                    <template #default="scope">
                        {{
                            scope.row.exists
                                ? scope.row.dims.length
                                    ? scope.row.dims.join(" / ")
                                    : "—"
                                : t("botSettings.vectorDimensionsNotCreated")
                        }}
                    </template>
                </el-table-column>
            </el-table>
            <template #footer>
                <el-button @click="vectorDimensionsVisible = false">
                    {{ $t("botSettings.confirm") }}
                </el-button>
            </template>
        </el-dialog>
        <div v-if="sentenceEmbeddingModelLoad.loading" class="model-load-status">
            {{
                $t("botSettings.hfModelLoading", {
                    model: hfModelLabel(
                        sentenceEmbeddingModelOptions,
                        sentenceEmbeddingModelLoad.model,
                    ),
                })
            }}
        </div>
        <div
            v-else-if="sentenceEmbeddingModelLoad.err"
            class="model-load-status is-error"
        >
            {{ sentenceEmbeddingModelLoad.err }}
        </div>
        <div v-if="isLocalEmbeddingModel" class="model-check-row">
            <el-button
                class="action-btn"
                v-if="canCheckEmbeddingModel"
                size="small"
                :loading="checkingEmbeddingModel"
                @click="
                    checkModelFiles(
                        'embedding',
                        settings.sentenceEmbeddingProvider.provider.model,
                    )
                "
            >
                {{ $t("botSettings.hfModelCheck") }}
            </el-button>
            <!-- 同对话卡片：重装按钮不看"模型缺失"提示，谁都能点。 -->
            <el-button
                class="action-btn"
                size="small"
                :loading="sentenceEmbeddingModelLoad.loading"
                @click="reloadModel('embedding')"
            >
                {{ $t("botSettings.hfModelReload") }}
            </el-button>
        </div>
        <el-alert
            v-if="showHfIncorrectEmbeddingModelTip"
            type="warning"
            :closable="false"
            class="hf-alert"
        >
            <template #title>
                {{ $t("botSettings.hfModelMissing") }}
                <el-button
                    type="primary"
                    text
                    @click="
                        downloadModels(
                            settings.sentenceEmbeddingProvider.provider.model,
                            'embedding',
                        )
                    "
                >
                    {{ $t("botSettings.hfModelDownloadLink") }}
                </el-button>
                {{ $t("botSettings.hfModelManual", { path: sentenceEmbeddingModelLocalPath }) }}
            </template>
        </el-alert>
        <div
            v-if="showHfEmbeddingModelDownloadProgress"
            class="download-progress"
        >
            <div class="download-progress-url">{{ $t("botSettings.downloading") }}: {{ downloadingUrl }}</div>
            <el-progress
                :percentage="Number(downloadingProgress) || 0"
                :stroke-width="14"
                striped
                striped-flow
            />
        </div>
    </el-card>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">
                {{ $t("botSettings.smtp.title") }}
            </div>
        </template>
        <el-form :model="settings" :label-width="formLabelWidth">
            <el-form-item :label="$t('botSettings.smtp.host')">
                <el-input v-model="settings.smtpHost" />
            </el-form-item>
            <el-form-item :label="$t('botSettings.smtp.username')">
                <el-input v-model="settings.smtpUsername" />
            </el-form-item>
            <el-form-item :label="$t('botSettings.smtp.password')">
                <el-input
                    v-model="settings.smtpPassword"
                    type="password"
                    show-password
                />
            </el-form-item>
            <el-form-item :label="t('botSettings.connTimeout')">
                <el-input-number
                    v-model="settings.smtpTimeoutSec"
                    :min="1"
                    :max="600"
                />
                <span class="form-item-suffix">{{ t("common.sec") }}</span>
            </el-form-item>
            <el-form-item
                :label="$t('botSettings.smtp.emailRegex')"
                label-width="200px"
            >
                <el-input
                    v-model="settings.emailVerificationRegex"
                    :placeholder="defaultEmailVerificationRegex"
                />
                <div class="form-item-help">
                    {{ $t("botSettings.smtp.emailRegexHelp") }}
                </div>
            </el-form-item>
            <el-form-item label="">
                <el-button :loading="loading" type="info" @click="smtpTest">
                    {{ $t("botSettings.smtp.test") }}
                </el-button>
                <el-alert
                    v-if="smtpPassed"
                    :title="$t('botSettings.smtp.testPassed')"
                    type="success"
                    class="smtp-alert"
                />
                <el-alert
                    v-if="smtpFailed"
                    :title="smtpFailedDetail"
                    type="error"
                    class="smtp-alert"
                />
            </el-form-item>
        </el-form>
    </el-card>

    <div class="settings-footer">
        <el-button type="primary" @click="save">
            {{ $t("common.save") }}
        </el-button>
        <el-button @click="goBack()">{{ $t("common.back") }}</el-button>
    </div>
</template>
<style scoped>
.settings-card {
    border-radius: 12px;
    margin-bottom: 20px;
}

.settings-card :deep(.el-card__header) {
    padding: 14px 20px 0;
    border-bottom: none;
}

.settings-card .section-title {
    margin: 0;
}

.usage-note {
    color: var(--el-text-color-secondary, #909399);
    margin-bottom: 8px;
    font-size: 13px;
}

.form-item-suffix {
    margin-left: 8px;
    color: var(--el-text-color-secondary, #909399);
}

.form-item-help {
    width: 100%;
    color: var(--el-text-color-secondary, #909399);
    font-size: 12px;
    line-height: 1.5;
    margin-top: 4px;
}

/* 「模型」那一行是下拉 + 「获取模型列表」按钮：el-form-item__content 默认
   flex-wrap: wrap，而 el-select 自身宽度是 100%，按钮会被挤到下一行。这里禁止
   换行，让下拉自己收缩，按钮和下拉留在同一行。 */
.model-row :deep(.el-form-item__content) {
    flex-wrap: nowrap;
    gap: 8px;
}

.model-row :deep(.el-select) {
    flex: 1;
    min-width: 0;
}

.model-row :deep(.el-button) {
    flex-shrink: 0;
}

/* 页面里的操作按钮（获取模型列表 / 检测模型 / 重新加载模型 / 检测已存向量维度）：
   默认的白底描边按钮夹在表单里和输入框几乎一样，给一层浅底色区分出「可点」的入口。 */
.action-btn {
    background: var(--el-color-primary-light-9, #ecf5ff);
    border-color: var(--el-color-primary-light-5, #a0cfff);
    color: var(--el-color-primary, #409eff);
}

.action-btn:not(.is-disabled):not(.is-loading):hover,
.action-btn:not(.is-disabled):not(.is-loading):focus {
    background: var(--el-color-primary-light-8, #d9ecff);
    border-color: var(--el-color-primary, #409eff);
    color: var(--el-color-primary, #409eff);
}

/* 禁用 / 加载中沿用 Element Plus 的灰态，别让按钮看起来还能点。 */
.action-btn.is-disabled,
.action-btn.is-disabled:hover {
    background: var(--el-fill-color-light, #f5f7fa);
    border-color: var(--el-border-color-lighter, #ebeef5);
    color: var(--el-text-color-placeholder, #a8abb2);
}

/* 重建索引进度：三行"来源 + 进度条 + 计数"。跑的时候每秒刷一次，所以尽量少动布局
   （只有进度条宽度和数字在变）。 */
.reindex-panel {
    margin-top: 8px;
    padding: 8px 10px;
    border: 1px solid var(--el-border-color-lighter, #ebeef5);
    border-radius: 4px;
    background: var(--el-fill-color-lighter, #fafafa);
}

.reindex-phase {
    font-size: 12px;
    color: var(--el-text-color-regular, #606266);
    margin-bottom: 6px;
}

.reindex-source {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--el-text-color-secondary, #909399);
}

.reindex-source-name {
    width: 150px;
    flex-shrink: 0;
}

.reindex-count {
    width: 190px;
    flex-shrink: 0;
    text-align: right;
    white-space: nowrap;
}

.proxy-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
}
/* 开关不参与收缩，"启用" 等文案保持单行显示 */
.proxy-row :deep(.el-switch) {
    flex-shrink: 0;
}

.proxy-row :deep(.el-switch__label),
.proxy-row :deep(.el-switch__label *) {
    white-space: nowrap;
}

.threshold-row {
    display: flex;
    align-items: center;
    gap: 6px;
}

/* 「检测模型 / 重新加载模型」按钮行：和下面的警告条/加载提示占同一个位置
   （模型存在时是按钮，缺失时是警告）。 */
.model-check-row {
    display: flex;
    gap: 8px;
    margin-top: 8px;
}

/* 后台装载模型：加载中一句灰字，失败就把后端给的原因（哪个文件不对）红字留在
   这里，旁边那条"模型缺失"警告仍然负责给下载入口。 */
.model-load-status {
    margin-top: 8px;
    font-size: 12px;
    color: var(--el-text-color-secondary, #909399);
    word-break: break-all;
}

.model-load-status.is-error {
    color: var(--el-color-danger, #f56c6c);
}

.hf-alert {
    margin-top: 8px;
}

/* 「检测已存向量维度」弹窗里的说明文字：来源列是文件+表+列，比较长，允许折行。 */
.vector-dims-hint {
    font-size: 12px;
    color: var(--el-text-color-secondary, #909399);
    margin-bottom: 8px;
}

.vector-dims-configured {
    font-size: 13px;
    margin-bottom: 8px;
}

.hf-alert :deep(.el-button) {
    padding: 0;
}

.download-progress {
    margin-top: 12px;
}

.download-progress-url {
    font-size: 12px;
    color: var(--el-text-color-secondary, #909399);
    margin-bottom: 4px;
    word-break: break-all;
}

.smtp-alert {
    margin-left: 12px;
    flex: 1;
}

.settings-footer {
    position: sticky;
    bottom: 0;
    z-index: 10;
    display: flex;
    gap: 4px;
    padding: 12px 20px;
    margin: 0 -20px;
    background: var(--el-bg-color, #fff);
    border-top: 1px solid var(--el-border-color-light, #e4e7ed);
}
</style>
