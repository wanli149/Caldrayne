# Caldrayne Online

**中文名：** 卡德雷恩 Online  
**核心代号：** Veldr

**语言版本：** [English](README.md) | [简体中文](README.zh-CN.md)

Caldrayne Online 是一个基于开源项目 Veloren 演化而来的体素动作冒险 RPG 项目。这个仓库用于承载 Caldrayne 品牌化、本地开发、内容定制以及后续发布工作。

## 项目定位

Caldrayne Online 不是从零重写的新项目，而是建立在 Veloren 现有开源基础之上的品牌化衍生项目。我们会保留必要的许可证义务、上游署名关系和技术来源说明，同时逐步建立自己的品牌与内容方向。

## 品牌信息

- 对外游戏品牌：`Caldrayne Online`
- 中文名：`卡德雷恩 Online`
- 核心代号：`Veldr`

## 文档

- 英文项目说明：[README.md](README.md)
- 模块 C 环境 / 分发 / 发布回滚基线：[CALDRAYNE_环境分发与发布回滚基线.zh-CN.md](CALDRAYNE_环境分发与发布回滚基线.zh-CN.md)

## 上游说明

本项目基于开源项目 Veloren 开发。Veloren 是一个使用 Rust 编写的开源多人联机体素 RPG。

在进行 Caldrayne Online 的品牌化、功能扩展和内容制作时，请不要移除仍然具有法律或归属要求的上游许可证、第三方声明、原始致谢和必要署名。

## 当前开发说明

当前仓库优先服务于本地开发与品牌整合，因此部分内部 crate 名称、运行时代号与构建标识仍会保留内部 `veldr` 命名，以维持工具链稳定性与实现连续性。

## 客户端安装边界

- 当前桌面客户端家族对外通过公开二进制 `caldrayne` 暴露。
- `Public` 与 `Dev` 是这一套技术客户端的两种运行模式，不是两套独立安装产品。
- `caldrayne-dev` 这类导出当前表示开发构建 profile，不表示第二个玩家桌面客户端。
- `veldr-voxygen`、`veldr-server-cli`、部分 crate 名与兼容性相关键名等内部技术标识，当前为了稳定工作区构建与工具链而保留；它们不是对外品牌面。
- `caldrayne-server-cli` 是独立的专用服务端运行体和运维二进制，不属于桌面客户端安装边界。

## 参考资料

在 Caldrayne 专属文档进一步完善之前，优先参考仓库内已有资料：

- 英文项目说明：[README.md](README.md)
- 协作规范：[CONTRIBUTING.md](CONTRIBUTING.md)
- Nix 与打包说明：[nix/README.md](nix/README.md)
- 模块 C 环境 / 分发 / 发布回滚基线：[CALDRAYNE_环境分发与发布回滚基线.zh-CN.md](CALDRAYNE_环境分发与发布回滚基线.zh-CN.md)

如果确实需要追溯上游引擎背景或实现来源，再单独查阅保留的 Veloren 参考资料，但不要把它们视为 Caldrayne 当前的官方社区入口。

## 仓库地址

- Caldrayne 仓库：<https://github.com/wanli149/Caldrayne>
- 问题反馈：<https://github.com/wanli149/Caldrayne/issues>

## 许可证

Caldrayne Online 目前仍沿用本仓库已有的开源许可证体系。详情请查看 [LICENSE](LICENSE) 以及仓库中保留的上游许可证与致谢内容。
