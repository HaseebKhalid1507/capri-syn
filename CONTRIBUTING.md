# Contributing to capri-syn

Thanks for wanting to make her better. Bug reports, fixes, docs and ideas are all welcome.

## Before you start

- **Bugs:** open an issue with the bug template. Her log is the most useful thing you can attach
  (where to find it is in the template).
- **Ideas and questions:** [Discussions](https://github.com/HaseebKhalid1507/capri-syn/discussions).
  For anything big, open a discussion or an issue first, so we agree on the shape before you write it.
- **Security:** don't open a public issue. See [SECURITY.md](SECURITY.md).
- **Art:** capri-syn ships no character art, and PRs must not add any. `tools/make-pack` assembles her
  pack on the user's machine from its sources. See [docs/media/NOTICE.md](docs/media/NOTICE.md).

## Build from source

The toolchain is pinned in `rust-toolchain.toml`; rustup picks it up.

**Linux** (Debian/Ubuntu package names; [docs/INSTALL.md](docs/INSTALL.md) lists the others):

```sh
sudo apt-get install -y pkg-config libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
  libx11-xcb-dev libxcb1-dev libxcb-shape0-dev libxcb-randr0-dev libxcb-xfixes0-dev \
  libfontconfig-dev libfreetype-dev libasound2-dev libssl-dev libzstd-dev
cargo build --locked
```

**Windows:** Visual Studio's C++ build tools, then `cargo build --locked`. Details in
[docs/WINDOWS.md](docs/WINDOWS.md).

She needs a running Synaps daemon to have a brain; [docs/INSTALL.md](docs/INSTALL.md) covers it.

## Before you open a PR

CI runs these on Linux and Windows, and they must pass:

```sh
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

- **One change per PR**, with a title that says what it does.
- **CHANGELOG:** add a line under `## Unreleased` for anything a user would notice.
- **Docs:** if you change behaviour, keys, settings or install steps, update the doc that describes it
  (README, `docs/`).
- **UI changes:** add the `shots` label and CI takes screenshots of her on Linux and Windows, so
  reviewers can see the change.
- **Nothing personal** in code, docs, tests or screenshots: no real names, hostnames, home paths or
  tokens.

PRs are squash-merged.

## Licence

capri-syn's code is MIT (see [LICENSE](LICENSE) and the exclusions in the README's Credits & legal).
By opening a PR you agree your contribution is released under the same licence.

## Conduct

Everyone here follows the [Code of Conduct](CODE_OF_CONDUCT.md).
