# HTTP 接口

所有接口同源，无需登录。JSON 请求使用 `Content-Type: application/json`。
错误通常返回 `{"error":"错误说明"}`；框架对不合法的 JSON 或请求参数可能返回纯文本。

## 消息

| 方法与路径 | 作用 |
| --- | --- |
| `GET /api/messages?before=123&limit=40&q=关键词` | 按 ID 倒序分页，搜索文字和附件名称 |
| `POST /api/messages` | 创建消息 |
| `GET /api/messages/{id}` | 读取一条消息及其附件 |
| `PATCH /api/messages/{id}` | 修改文字，附件保持不变 |
| `DELETE /api/messages/{id}` | 删除消息及它的附件引用，返回 204 |

创建消息示例：

```json
{
  "client_id": "调用方为这次发送生成的唯一标识",
  "text": "文字原文",
  "attachments": ["已完成上传的文件 ID"],
  "reply_to": null
}
```

同一个 `client_id` 重试会返回已创建的消息。附件必须上传完成并通过校验。
`attachments` 可以包含多个文件 ID，同一个文件 ID 也可出现在多条消息中。
同一条消息不能重复列出同一个 ID。任何附件关联失败都会回滚整个消息事务。
删除消息会移除它的引用；其他消息或未发送附件仍使用相同内容时，原文件继续保留。
修改文字的请求体是 `{"text":"新的文字"}`。

分页响应包含 `messages`、`next_before` 和 `cursor`。
`next_before` 为 `null` 表示没有更早的结果。`cursor` 是获取快照时的最新事件序号。

## 实时同步

`GET /api/events?after={cursor}` 返回 SSE 事件流，每 15 秒发送保活注释。
自动重连时，`Last-Event-ID` 请求头优先于 `after` 参数。

```text
event: change
id: 42
data: {"id":42,"kind":"upsert","message_id":18}
```

`upsert` 表示获取这条消息的最新内容，`delete` 表示移除这条消息。
事件只携带标识，不重复传输消息正文和文件内容。
事件从 SQLite 重放，客户端应按消息 ID 合并，允许重复通知。
如果收到 `reset` 事件，重新获取分页快照和事件流。

## 附件上传

1. 在客户端计算完整文件的 SHA-256，然后 `POST /api/uploads`，请求体为 `{"name":"文件名","size":文件字节数,"sha256":"64位十六进制摘要"}`。摘要必填，接受大小写。
2. 从响应获取 `id`。如果 `sha256` 非空，说明已命中相同摘要和大小的已验证内容，可跳过上传及完成接口。从 `GET /api/config` 获取 `chunk_size`（8 MiB）和 `upload_concurrency`（4）。
3. `PUT /api/uploads/{id}/chunks/{offset}`，正文是原始分片。每个网页同时发送最多 4 个分片，可以属于同一个文件。
4. 分片起点为 8 MiB 的整数倍，正文长度必须为 `min(8 MiB, 文件大小 - offset)`。只有最后一片可以更短。服务器同步写盘并提交进度后确认该片。
5. `POST /api/uploads/{id}/complete`，服务器读取原文件计算 SHA-256，并与创建上传时声明的摘要比较。不一致返回 422，清空已确认进度和分片记录，客户端可从头重试。
6. 客户端确认返回的 `sha256` 与本地摘要一致后，创建消息，在 `attachments` 中引用文件 ID。

每次创建上传返回独立的附件 ID，保留本次文件名和 MIME 类型。秒传附件在底层引用同一份内容，不复制文件。
这样可以独立取消未发送附件，不影响其他网页的草稿或已发送消息。
多个上传同时声明同一摘要时，尚未验证的上传不能互相秒传；各自校验通过后，存储合并为一份。
旧数据目录中尚未完成的上传没有客户端摘要，继续按旧协议完成服务器校验；新建上传必须声明摘要。

`GET /api/uploads/{id}` 可查询进度。`received` 是从文件开头连续确认的字节数，`uploaded` 是所有已确认分片的总字节数。
分片可以乱序完成，不能把 `uploaded` 当成下次上传的起点。
请求失败或响应丢失时，先等待当前批次结束，再查询进度，从 `received` 继续发送。
重复发送已经确认的分片会返回当前进度，不覆盖原字节。失败分片不会提交进度，也不会截断其他分片。
未填满的空洞不能通过完成检查。乱序进度保存在 SQLite，重启后继续有效。

原有 `PATCH /api/uploads/{id}` 接口仍可使用：请求头 `Upload-Offset` 等于 `received`，正文长度为 1 字节到 8 MiB。
此接口按顺序上传，偏移不一致或存在已确认的乱序分片时返回 409。重试时截断未确认的尾部。
从旧接口切换到并发接口时，第一片可以从非对齐的 `received` 开始，但必须到下一个 8 MiB 边界或文件末尾结束。
空文件直接调用完成接口。完成接口可以重复调用。

`DELETE /api/uploads/{id}` 取消尚未关联消息的附件，释放它的存储引用。
仍被消息引用的附件返回 409，应删除对应消息。原文件只在最后一个存储引用释放后清理。

上传对象包含 `id`、`name`、`size`、`mime`、`received`、`uploaded` 和 `sha256`。
`sha256` 在完成前为 `null`，完成或秒传后为 64 位小写十六进制字符串。
文件长度使用 64 位整数；网页能处理的实际长度还受浏览器和文件系统影响。

## 附件下载与预览

`GET /api/files/{id}/content` 直接返回原文件，支持 `HEAD` 和 `Range`。
加上 `?download=1` 会设置附件下载响应头，并提供 UTF-8 原始文件名。

```http
Range: bytes=0-65535
```

有效范围返回 206 和 `Content-Range`，不可满足的范围返回 416。
还未完成校验的文件返回 409，已删除或不存在的文件返回 404。

## 运行信息

- `GET /api/health`：返回 `ok`。
- `GET /api/config`：返回 `chunk_size`、`upload_concurrency` 和程序 `version`。
- `homeclip --healthcheck`：用内置 HTTP 客户端检查当前监听地址，供 `scratch` 镜像健康检查使用。
