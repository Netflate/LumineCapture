# Contributing

LumineCapture is open to contributions from anyone ! 

## Coordination

If you have questions, suggestions, ideas, you can open an issue to discuss it, or add your message in an already existing issue if it fits its scope.
For bigger changes, open an issue first so we can agree on the approach before you write the code.

## Building

You need Rust, a C toolchain, clang, pkg-config and the development headers of pipewire, libxkbcommon and openssl. On Fedora:

```sh
sudo dnf install clang pipewire-devel libxkbcommon-devel openssl-devel
cargo run --release
```

Always use `--release`: otherwise it will be slow.

CI runs `./scripts/ci.sh quick` (fmt, clippy, tests). To run it yourself before every push: `git config core.hooksPath .githooks`.

Commit messages are `area: what changed`, for example `toolbar: fix tooltip position`. Keep one change per commit.

## AI policy

AI should be used as a tool, not as a replacement for your own work: you must understand every line of code you submit and be able to explain it.

It is your responsibility to ensure that:
- You fully understand how the code in your contribution works.
- Your change is accurately described in the commit message.
- Your contribution is tested and working as described.
- To the best of your knowledge, you are not introducing bugs; any exceptions to this rule must be presented with clear reasoning.
- Any use of AI is indicated in the PR description.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as MIT or Apache-2.0, without any additional terms or conditions.

---

Adapted from Smithay's [CONTRIBUTING.md](https://github.com/Smithay/smithay/blob/master/CONTRIBUTING.md)
and [AI.md](https://github.com/Smithay/smithay/blob/master/AI.md) (MIT).
