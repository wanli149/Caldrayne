# Caldrayne Online

**中文名：** 卡德雷恩 Online  
**核心代号：** Veldr

**Languages:** [English](README.md) | [简体中文](README.zh-CN.md)

Caldrayne Online is a voxel action-adventure RPG project built on top of the open-source Veloren codebase. This repository is the Caldrayne-branded workspace for local development, customization, and future publishing.

## Project Positioning

Caldrayne Online is not a from-scratch rewrite. It is a branded derivative project based on Veloren, and it keeps the original open-source foundation, license obligations, and upstream technical heritage intact.

## Branding

- Public game brand: `Caldrayne Online`
- Chinese name: `卡德雷恩 Online`
- Core codename: `Veldr`

## Documentation

- Chinese project overview: [README.zh-CN.md](README.zh-CN.md)
- Module C environment, distribution, and rollback baseline: [CALDRAYNE_环境分发与发布回滚基线.zh-CN.md](CALDRAYNE_环境分发与发布回滚基线.zh-CN.md)

## Upstream Credit

This project is based on the open-source Veloren project, a multiplayer voxel RPG written in Rust.

We retain the required upstream license and attribution material in this repository. When working on Caldrayne-specific features, please avoid removing or obscuring original license notices, third-party credits, or upstream acknowledgements that are still required.

## Development Notes

This repository currently prioritizes local development and brand integration. Some internal crate names, runtime codenames, and build identifiers still use the internal `veldr` naming where keeping them preserves tooling stability and implementation continuity.

## Client Packaging Boundary

- The current desktop client family is exposed through the public binary `caldrayne`.
- `Public` and `Dev` are two runtime product modes of that same client family, not two separate install products.
- Outputs such as `caldrayne-dev` represent development build profiles, not a second player-facing desktop client.
- Internal package identifiers such as `veldr-voxygen`, `veldr-server-cli`, selected crate names, and compatibility-oriented keys are still retained where they stabilize the workspace. They are not the public brand surface.
- `caldrayne-server-cli` is a separate dedicated server runtime and operational binary, not part of the desktop client install boundary.

## Reference Material

Until Caldrayne-specific documentation is expanded, use the repository materials first:

- Project contribution guide: [CONTRIBUTING.md](CONTRIBUTING.md)
- Nix and packaging notes: [nix/README.md](nix/README.md)
- Chinese project overview: [README.zh-CN.md](README.zh-CN.md)
- Module C environment, distribution, and rollback baseline: [CALDRAYNE_环境分发与发布回滚基线.zh-CN.md](CALDRAYNE_环境分发与发布回滚基线.zh-CN.md)

When upstream engine history or implementation background is needed, consult preserved Veloren references separately without treating them as Caldrayne community entry points.

## Repository

- Caldrayne repository: <https://github.com/wanli149/Caldrayne>
- Issue tracker: <https://github.com/wanli149/Caldrayne/issues>

## License

Caldrayne Online remains distributed under the repository's existing open-source licensing terms. See [LICENSE](LICENSE) and preserved upstream notices for details.
