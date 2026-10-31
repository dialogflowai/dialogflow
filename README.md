[简体中文](./README_zh-CN.md) | English

# Dialog flow AI
**only ONE executable file**, you can use it directly, including intent detection, AI management, a visual process editor and a response system.  
**ZERO installation**, no need to install any middleware such as Redis, Elastic Search, etc.  
<img src="https://img.shields.io/badge/Latest_version-v1.23.0-blue" /> <img src="https://img.shields.io/badge/MSRV-1.88.0-green" /> [![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/dialogflowai/dialogflow)

![All in ONE](./doc/assets/allinone.png)

![Dialog flow editor](./doc/assets/screenshots/flow-editor-en.png)

![Dialog flow editor](./doc/assets/screenshots/chat_demo_eng.gif)

# ✨ Features
* 🛒 **Light** Only ONE executable file, it can run smoothly on laptops without GPUs (data files will be created at runtime automatically).
* 🐱‍🏍 **AI powered** Integrated `Huggingface local models (Llama, Phi-3, Gemma, Multilingual E5, MiniLM L6v2, NomicEmbedTextV1_5, etc.)`, and **any OpenAI-compatible endpoint** — `OpenAI`, `DeepSeek`, `Zhipu GLM`, `Qwen`, `Moonshot Kimi`, `SiliconFlow`, `Groq`, `OpenRouter`, `Ollama`, `vLLM`, `LM Studio`, or your own gateway. Just fill in the URL and API key. This can be used for `Chat`, `Text generation` and `Intent detection`.
* 🚀 **Fast** Built on Rust and Vue3.
* 😀 **Simple** Use the mouse to drag and drop with our intuitive node-based editor.
* 🔐 **Safe** 100% open source, all runtime data is saved locally (Using `OpenAI API` may expose some data).

# Give it a try!
* 🐋 **Docker** We provided an image on Docker Hub at [dialogflowai/dialogflow](https://hub.docker.com/r/dialogflowai/dialogflow/)
* 💻 **Binary releases**, please check [here](https://github.com/dialogflowai/dialogflow/releases)

> By default application will listen to `127.0.0.1:12715`, you can use `-ip` and `-port` specify new value, e.g.: `dialogflow -ip 0.0.0.0 -port 8888`

### Streaming answers

`POST /flow/answer` returns the answer in one document unless the request body asks
for `"stream": true`. Then the response is `application/x-ndjson`: one JSON frame per
line, each carrying a piece of an answer (`{"contentSeq": 0, "content": "..."}`), and
the last line is always a terminal frame (`contentSeq: null`) whose `content` is the
whole `{status, data, err}` response, so the final `nextAction` and `collectData`
arrive with it.

```bash
curl -N -H 'Content-Type: application/json' \
  -d '{"robotId":"...","mainFlowId":"...","userInput":"hi","stream":true}' \
  http://127.0.0.1:12715/flow/answer
```

A client that does not send `stream` gets exactly the JSON it got before 1.23. See
[doc/streaming.md](doc/streaming.md) for the full protocol, the Java/JavaScript client
contracts and the known limitations.

<!-- # Releases and source code
* 💾 If you're looking for **binary releases**, please check [here](https://github.com/dialogflowai/dialogflow/releases)
* 🎈 The **back end** of this application is [here](https://github.com/dialogflowchatbot/dialogflow-backend)
* 🎨 The **front end** of this application is [here](https://github.com/dialogflowchatbot/dialogflow-frontend) -->

# Check out introduction page
[https://dialogflowai.github.io/](https://dialogflowai.github.io/)

# Function nodes
|Node|Name|
|----|----|
|![DialogNode](./doc/assets/screenshots/dialogNode.png)|Dialog Node|
|![LLM chat node](./doc/assets/screenshots/llmChatNode.png)|Large language model chat node|
|![](./doc/assets/screenshots/knowledgeBaseAnswerNode.png)|Knowledge base answer node|
|![](./doc/assets/screenshots/conditionNode.png)|Conditions node|
|![](./doc/assets/screenshots/gotoNode.png)|Goto node|
|![](./doc/assets/screenshots/collectNode.png)|Collect node|
|![](./doc/assets/screenshots/externalApiNode.png)|External HTTP node|
|![](./doc/assets/screenshots/sendEmailNode.png)|Send email node|
|![](./doc/assets/screenshots/theEndNode.png)|The end node|

Using the different nodes above, to arrange and combine, you can get a conversational bot that can handle problems in different scenarios.

# Screenshots
![Homepage](./doc/assets/screenshots/homepage.png)

![Robot detail](./doc/assets/screenshots/robotDetail.png)

### Trying a demo dialog flow
![Demo](./doc/assets/screenshots/demo1.gif)

### Setup a condition branch
![Setup a condition branch](./doc/assets/screenshots/condition1.gif)

### Text generation

![Text generation](./doc/assets/screenshots/textGeneration.gif "Text generation")

### Testing a dialog flow
![Flow testing tool](./doc/assets/screenshots/testing.png "Flow testing tool")


# Get started

### Docker image
1. docker pull dialogflowai/demo
2. docker run -dp 127.0.0.1:12715:12715 --name dialogflowdemo dialogflowai/demo
3. Open your browser and visit: http://127.0.0.1:12715/

### Binary release
1. From [Github release page](https://github.com/dialogflowai/dialogflow/releases), depending on the operating system, download the application.
1. Run it directly, or use the `-ip` and `-port` parameters to perform the listening IP address and port, e.g.: `dialogflow -ip 0.0.0.0 -port 8888`.
1. Open your browser and visit http://localhost:12715 (by default) or http://`new IP`:`new port` to see the application in action
1. Add a main flow and click its name into it
1. Create dialog flow by dragging and drop nodes onto canvas
1. Test it

## We also provide Enterprise and Government & Enterprise Editions

| Feature | Community Edition | Enterprise Edition | Government & Enterprise Edition |
|---|---|---|---|
| Visual node editor | Supported | Supported | Supported |
| Dialogue nodes | Supported | Supported | Supported |
| LLM chat nodes | Supported | Supported | Supported |
| Knowledge base answer nodes | Supported | Supported | Supported |
| Condition nodes | Supported | Supported | Supported |
| Collection nodes | Supported | Supported | Supported |
| Jump nodes | Supported | Supported | Supported |
| External HTTP request nodes | Supported | Supported | Supported |
| Email sending nodes | Supported | Supported | Supported |
| End nodes | Supported | Supported | Supported |
| Intent recognition | Supported | Supported | Supported |
| Knowledge base | Supported | Supported | Supported |
| Variable support | Supported | Supported | Supported |
| Create dialogue flows via API | Not supported | Supported | Supported |
| Data export/import | Not supported | Supported | Supported |
| SQL Server | Not supported | WIP | Supported |
| SQLite | Not supported | WIP | Supported |
| PostgreSQL | Not supported | Supported | Supported |
| MySQL | Not supported | WIP | Supported |
| Audit | Not supported | Supported | Supported |
| Xinchuang / localized ecosystem | Not supported | Not supported | Supported |
| License | Apache License 2.0 | Apache License 2.0 | Apache License 2.0 |
| Technical support | GitHub issues | Email | Email |
| Project link | https://github.com/dialogflowai/dialogflow | https://github.com/Braidwork/Kiko | Email: dialogflow@yeah.net |
