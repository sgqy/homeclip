# Homeclip

家庭局域网共享消息板。在 PC 和手机浏览器之间传递文字和原始文件。

项目正在开发。后端使用 Rust、Axum 和 SQLite，前端使用 Vue 3 和 TypeScript。
最终镜像以 `scratch` 为基础，包含静态链接程序和前端资源，数据挂载到 `/data`。

## 开发

```sh
cargo run
```

默认监听 `0.0.0.0:8080`，健康检查为 `GET /api/health`。
使用环境变量 `HOMECLIP_BIND` 修改监听地址。

## 许可证

MIT，见 [LICENSE](LICENSE)。
