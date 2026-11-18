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
| `DELETE /api/messages/{id}` | 删除消息及附件，返回 204 |

创建消息示例：

```json
{
  "client_id": "调用方为这次发送生成的唯一标识",
  "text": "文字原文",
  "attachments": ["已完成上传的文件 ID"],
  "reply_to": null
}
```

同一个 `client_id` 重试会返回已创建的消息。附件必须上传完成且尚未关联其他消息。
失败的附件关联会回滚整个消息事务。
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

1. `POST /api/uploads`，请求体为 `{"name":"文件名","size":文件字节数}`。
2. 从响应获取 `id` 和 `received`。
3. `PATCH /api/uploads/{id}`，请求头 `Upload-Offset` 等于已确认的字节数，正文是原始分片。
4. 重复发送，每片最多 8 MiB。服务器写盘并提交进度后返回新的 `received`。
5. `POST /api/uploads/{id}/complete`，等待原文件 SHA-256 计算完成。
6. 创建消息，在 `attachments` 中引用文件 ID。

`GET /api/uploads/{id}` 可查询进度。请求失败或响应丢失时，先查询进度，再从已确认的位置继续。
偏移不一致返回 409。中断请求留下的未确认字节会在重试时截断。
空文件直接调用完成接口。完成接口可以重复调用。

`DELETE /api/uploads/{id}` 取消尚未关联消息的上传并删除磁盘文件。
已关联消息的附件不能通过这个接口删除，应删除对应消息。

上传对象包含 `id`、`name`、`size`、`mime`、`received` 和 `sha256`。
`sha256` 在完成前为 `null`，完成后为 64 位十六进制字符串。
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
- `GET /api/config`：返回 `chunk_size` 和程序 `version`。
- `homeclip --healthcheck`：用内置 HTTP 客户端检查当前监听地址，供 `scratch` 镜像健康检查使用。
