# view_watch 可视化

使用静态 TypeScript 页面和 PostgREST 查看 `public.view_watch` 原始行,
按文件 SHA-256 修改 `public.media_file.ddp_epi_id`. 浏览器不保存数据库连接
或认证信息, 不访问数据库 TCP 端口. 项目不实现自定义后端.

## 部署和安全

两个 Compose 服务分别运行 Nginx 静态站点和官方 PostgREST. 仅 Nginx
绑定宿主机 `127.0.0.1:${PORT}`, PostgREST 不暴露宿主机端口.
外部访问必须通过可信反向代理提供 HTTPS, 认证和访问控制.
应用本身不认证, 能访问站点的用户都能修改所有文件的该字段.
受信用户也可以直接调用 REST 接口批量修改该列, 不受页面单行操作限制.
同源资源必须可信, 防止通过 XSS 利用用户的写入权限.

复制 `.env.example` 为 `.env`, 设置 `PORT` 和 `PGRST_DB_URI`.
URI 中用户名或密码包含保留字符时需按 URL 规则百分号编码.
`.env` 不复制到镜像, 不提交到版本控制. Compose 配置或容器环境中
仍可能被有宿主机访问权限的用户读取, 需限制权限并采用部署平台密钥管理.

```bash
cd view_watch
cp .env.example .env
# 编辑 .env 并配置 PostgreSQL 连接.
docker compose config --quiet
docker compose up -d --build
curl --fail http://127.0.0.1:56082/health
```

配置外部反向代理转发到 `127.0.0.1:56082`, 保护整个站点和 `/rest/`.
改变 `.env` 后使用 `docker compose up -d` 重新创建容器,
只执行 `restart` 不会更新容器环境.

## 数据库权限

在隔离环境审阅 `database/grants.sql`, 然后由管理员在目标数据库执行:

```bash
psql -v ON_ERROR_STOP=1 -d main -f database/grants.sql
```

脚本创建无密码的 `watch_gateway` 登录角色和 `watch_user` 非登录角色,
将后者授予前者. 登录角色的密码应由管理员使用 `psql` 的 `\password`
交互设置, 再写入部署环境. 用户必须具备数据库 CONNECT 权限.

脚本仅授予 `view_watch` 查询, `media_file.sha256/ddp_epi_id` 查询
及 `ddp_epi_id` 单列更新权限, 不创建 schema, 表或视图.
角色还可能继承 PostgreSQL `PUBLIC` 或已有角色权限; 上线前必须检查
并撤销意外的权限. 不要将管理员账号作为 PostgREST 匿名角色.
在已有角色同名时脚本不会删除或收窄它的既有权限.

## 页面使用

搜索, 筛选和排序由 PostgREST 在数据库中执行. 默认每页 100 行,
可选择每页 20, 50, 100, 200, 500 或 1000 行.
页面固定使用高对比度深色主题, 每 10 秒刷新; 页面隐藏时暂停.
统计, 筛选, 附加列控制和分页导航位于同一工具栏.
拖动表头右侧边缘调整列宽, 设置保存在当前浏览器. 双击边缘可重置单列,
也可使用表格上方的重置按钮. 聚焦调整手柄后可用方向键调整,
按住 Shift 加大步长, Home 恢复默认. 每列宽度范围为 60 至 4000 像素;
浏览器禁止本地存储时, 页面会提示设置仅在当前页面有效.
点击每行状态按钮可展开全部原始字段, 再次点击可收起.
表格中截断的文本可通过鼠标悬浮查看完整内容.

只要行有 `file_sha256`, 即可点击编辑. 输入留空保存为 SQL NULL,
输入 `-1` 表示无法关联, `0` 和正整数按数据库现有约束保存.
剧集 ID 支持 PostgreSQL bigint 范围, 不受 JavaScript 整数精度限制.
提交后直接更新 `media_file`, 重新查询 `view_watch` 显示数据库结果.
重复文件行会随重查更新; 无文件哈希的行只读.
多用户并发时后一次成功保存会覆盖前一次, 本应用不提供冲突检测.

## 开发

安装前端依赖并启动 Vite:

```bash
npm ci --ignore-scripts --no-audit --no-fund
npm run build
npm run dev
```

开发代理默认将 `/rest` 转发到 `http://127.0.0.1:3000`.
可通过开发进程的 `WATCH_DEV_REST` 指定本机 PostgREST 地址;
此地址只用于 Vite 代理, 不进入前端构建产物.
Compose 默认不暴露 PostgREST, 因此独立开发时需另行启动本机实例.

## 故障排查

- 页面显示连接错误: 检查 PostgREST 容器日志, 数据库地址和角色权限.
- 视图查询被拒绝: 检查 `public.view_watch` SELECT 和 schema USAGE.
- 保存返回权限错误: 检查 `media_file.sha256` SELECT 及 `ddp_epi_id` UPDATE.
- 保存显示文件不存在: 检查视图 SHA-256 是否对应 `media_file` 和 bytea 筛选.
- ID 已保存但无番剧信息: 当前 ID 不在关联的剧集数据中, 不表示保存失败.
- 存在 PUBLIC 过宽授权: 检查角色的实际有效权限, 不要依赖授权脚本收回其他权限.
