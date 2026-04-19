# Caldrayne 技术标识品牌化评估清单

这份清单只评估高风险技术标识，不代表本轮立即修改。

当前仓库已经完成了大部分对外品牌化，但仍保留一批历史 `veloren`
技术标识，用于保证构建、部署、兼容性和工具链稳定。下面按风险分组。

## 1. 可以改，但必须整组一起改

### 1.1 Desktop / App ID 链

当前状态：

- 已执行第一阶段迁移
- 当前推荐并已采用的新 ID 为 `io.github.wanli149.caldrayne`

当前关键位置：

- `voxygen/src/window.rs`
- `voxygen/Cargo.toml`
- `assets/voxygen/io.github.wanli149.caldrayne.desktop`
- `assets/voxygen/io.github.wanli149.caldrayne.metainfo.xml`
- `assets/voxygen/io.github.wanli149.caldrayne.png`

现状：

- Wayland 窗口类名已迁移到 `io.github.wanli149.caldrayne`
- desktop 文件名、metainfo id、launchable、icon 名已统一迁移
- `Exec` 和 metainfo 里的 binary 名仍保留 `veloren-*`

风险判断：

- 这是一整条 Linux 桌面集成链
- 不能只改其中一个点，否则会出现启动器、图标、AppStream、Wayland 类名不一致
- 新 ID 选择 GitHub 命名空间是为了避免假定拥有尚未确认的独立域名

建议：

- 这一批可以先改，并且应保持和二进制名迁移解耦
- 修改时要同步改文件名、Cargo metadata、desktop id、icon id、launchable id
- 在未迁移二进制名前，保留 `Exec=veloren-voxygen` 与 metainfo binary 现状

### 1.2 Mumble / IPC 标识

当前关键位置：

- `voxygen/src/session/mod.rs`

现状：

- 仍使用 `SharedLink::new("veloren", "veloren-voxygen")`

风险判断：

- 这会影响外部语音 / IPC 集成识别
- 改动范围虽小，但属于兼容性变化

建议：

- 可以改
- 适合放进 Desktop / 二进制命名专门批次中一起处理

## 2. 暂时不建议现在改

### 2.1 二进制名

当前关键位置：

- `voxygen/Cargo.toml`
- `server-cli/Cargo.toml`
- `.cargo/config.toml`
- `server-cli/Dockerfile`
- `assets/voxygen/io.github.wanli149.caldrayne.desktop`
- `assets/voxygen/io.github.wanli149.caldrayne.metainfo.xml`
- `flake.nix`
- `nix/README.md`

现状：

- 仍使用 `veloren-voxygen`
- 仍使用 `veloren-server-cli`

风险判断：

- 这不只是 Cargo 包名，还连着 Docker COPY/ENTRYPOINT、Nix 包输出、desktop Exec、
  metainfo binary、cargo alias、构建脚本
- 改动面大，而且一旦漏掉一个点，构建或部署就会直接坏掉

建议：

- 现在不要直接改
- 如果要改，必须先做一次“全链路二进制命名迁移”

### 2.2 crate / package 名

当前关键位置：

- `voxygen/Cargo.toml`
- `voxygen/anim/Cargo.toml`
- `voxygen/egui/Cargo.toml`
- `voxygen/i18n-helpers/Cargo.toml`
- 多个 `Cargo.toml` 中的 `package = "veloren-*"`
- `Cargo.lock`

现状：

- 仍保留 `veloren-voxygen`、`veloren-voxygen-anim`、`veloren-voxygen-egui`、
  `veloren-voxygen-i18n-helpers` 等命名

风险判断：

- 会联动 workspace 依赖、锁文件、Nix 构建、Cargo alias、子 crate 引用
- 风险比文案替换高得多

建议：

- 当前阶段不要改
- 需要在构建链和部署链稳定后再做

### 2.3 环境变量与构建常量

当前关键位置：

- `common/build.rs`
- `flake.nix`
- `common/assets/src/fs.rs`

现状：

- 仍使用 `VELOREN_GIT_VERSION`
- 仍使用 `VELOREN_ASSETS`
- 仍使用 `VELOREN_USERDATA_STRATEGY`
- canary magic 仍为 `VELOREN_CANARY_MAGIC`

风险判断：

- 这些变量已经深入构建和运行时逻辑
- 简单替换名称会破坏脚本、打包、Nix 包装层和运行时资源探测

建议：

- 暂时不要改
- 若后续一定要品牌统一，建议先做兼容层，再逐步迁移

### 2.4 Flake / Nix 输出名

当前关键位置：

- `flake.nix`
- `nix/README.md`

现状：

- 仍输出 `packages.veloren-voxygen`
- 仍输出 `packages.veloren-server-cli`
- `nci.crates` 里仍是 `veloren-*`

风险判断：

- 这部分直接跟二进制名、crate 名绑定
- 单独改 Nix 层意义不大，反而会造成命名不一致

建议：

- 跟二进制名迁移绑定处理
- 不建议单独先改

### 2.5 插件接口命名 / ABI

当前关键位置：

- `plugin/wit/veloren.wit`

现状：

- WIT package 仍为 `veloren:plugin@0.0.1`

风险判断：

- 这是插件接口命名空间，不只是文件名问题
- 改动可能影响插件 ABI、代码生成结果和外部插件兼容

建议：

- 现在不要改
- 等插件体系明确对外发布策略后再决定

## 3. 低收益，优先级后置

### 3.1 内部类型名 / 变量名

示例位置：

- `server/src/persistence/mod.rs` 中的 `VelorenConnection`
- `server/src/persistence/character/conversions.rs` 中的 `VelorenItem`

风险判断：

- 技术上大多可改
- 但对用户不可见，收益很低

建议：

- 除非后续正好在重构相关模块，否则不值得单独为品牌化动它们

### 3.2 旧版本存档 / 世界版本枚举名

示例位置：

- `world/src/sim/mod.rs`

现状：

- 仍有 `Veloren0_5_0`、`Veloren0_7_0`

风险判断：

- 这些名称承载历史存档格式语义
- 品牌化收益远小于兼容性风险

建议：

- 不建议改

## 4. 推荐执行顺序

### 批次 A：Desktop / App ID 专项

目标：

- 统一 Linux 桌面集成标识

包含：

- `net.veloren.veloren` -> `io.github.wanli149.caldrayne`
- desktop 文件名
- metainfo id
- Wayland 窗口类名
- 相关 icon id

当前执行结论：

- 已完成
- 本批次故意未修改 `Exec=veloren-voxygen`
- 本批次故意未修改 metainfo 内的 binary 名
- 本批次故意未触碰 crate 名、Cargo 包名、Nix 输出名

### 批次 B：二进制名与打包链专项

目标：

- 评估是否把 `veloren-voxygen` / `veloren-server-cli` 迁移到
  `caldrayne-*`

包含：

- Cargo 包名
- Cargo alias
- Dockerfile
- Nix / flake 输出名
- desktop Exec
- metainfo binary

### 批次 C：环境变量与插件 ABI

目标：

- 只在确实需要彻底去 `VELOREN_*` 时执行

包含：

- 构建环境变量
- canary magic
- WIT package 命名空间

## 5. 当前建议结论

- `Desktop ID`：已完成第一阶段整组迁移，当前统一为 `io.github.wanli149.caldrayne`
- 二进制名：现在先别改，等做完整的打包链迁移时再改
- crate 名：现在先别改
- `VELOREN_*` 环境变量：现在先别改
- `plugin/wit/veloren.wit`：现在先别改
- 内部 `Veloren*` 类型名：可以长期逐步清理，但优先级最低

## 6. 已执行记录

### 2026-04-19

- 已将 Linux Desktop / App ID 从 `net.veloren.veloren` 迁移到
  `io.github.wanli149.caldrayne`
- 已同步迁移：
  - `voxygen/src/window.rs`
  - `voxygen/Cargo.toml`
  - desktop 文件名
  - metainfo 文件名
  - desktop / metainfo 中的 id、launchable、icon 名
  - 对应 PNG 图标文件名
- 本次明确保留：
  - `veloren-voxygen`
  - `veloren-server-cli`
  - metainfo `<binary>` 名称
  - Cargo / Nix / Docker 构建链中的 `veloren-*` 技术标识
