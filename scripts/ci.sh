#!/usr/bin/env bash
# GitHub Actions (.github/workflows/ci.yml) runs `quick` on every push
# the rest is for running by hand before a release.
#
#   ./scripts/ci.sh quick   fmt + clippy + tests
#   ./scripts/ci.sh full    quick + ocr-gpu + machete + deny + assets
#   ./scripts/ci.sh rpm     dist build -> RPM -> inspect the payload
#   ./scripts/ci.sh clean   build RPM in a fresh Fedora container,
#                           then test installation in a bare container.
#   ./scripts/ci.sh compat  release packages: rpm and deb, then `arch`
#   ./scripts/ci.sh arch    PKGBUILD 
#
# Enable the pre-push hook once per clone (hooks are not cloned by git):
#   git config core.hooksPath .githooks

set -euo pipefail
cd "$(dirname "$0")/.."

readonly IMAGE="registry.fedoraproject.org/fedora:44"
# pipewire, xkbcommon, openssl, and clang for bindgen
readonly BUILD_DEPS="gcc gcc-c++ clang pkgconf-pkg-config pipewire-devel libxkbcommon-devel openssl-devel"

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
skip() { printf '\033[1;33m--- skipped: %s\033[0m\n' "$*"; }
note() { printf '\033[1;33m--- note: %s\033[0m\n' "$*"; }
die()  { printf '\033[1;31mFAILED: %s\033[0m\n' "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

quick() {
    step "rustfmt"
    cargo fmt --check

    step "clippy (warnings are errors)"
    cargo clippy --all-targets --locked -- -D warnings

    step "tests"
    cargo test --locked
}

full() {
    quick

    step "clippy + tests with ocr-gpu"
    # the GPU path is a non-default feature, so nothing above ever compiles it
    cargo clippy --all-targets --locked --features ocr-gpu -- -D warnings
    cargo test --locked --features ocr-gpu

    step "unused dependencies"
    if have cargo-machete; then
        cargo machete
    else
        skip "cargo-machete (cargo install cargo-machete)"
    fi

    step "desktop entry"
    if have desktop-file-validate; then
        desktop-file-validate assets/lumine-capture.desktop
    else
        skip "desktop-file-validate (dnf install desktop-file-utils)"
    fi
    grep -q '^X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2$' \
        assets/lumine-capture.desktop \
        || die "the .desktop lost its KWin screenshot grant"

    step "licences and advisories"
    if have cargo-deny; then
        cargo deny check
    else
        skip "cargo-deny (cargo install cargo-deny --locked)"
    fi
}

package_rpm() {
    have cargo-generate-rpm || die "cargo-generate-rpm is missing (cargo install cargo-generate-rpm --locked)"

    local target_dir="${CARGO_TARGET_DIR:-target}"
    local out="$target_dir/generate-rpm"

    step "dist build (full LTO, takes a few minutes)"
    cargo build --profile dist --locked

    step "building the RPM"
    rm -rf "$out"
    cargo generate-rpm --profile dist --target-dir "$target_dir"

    local package
    package=$(find "$out" -name '*.rpm' -print -quit 2>/dev/null || true)
    [ -n "$package" ] || die "cargo-generate-rpm produced no package in $out"

    step "payload of $(basename "$package")"
    local payload
    payload=$(command rpm -qpl "$package")
    printf '%s\n' "$payload"
    # the three files that make the app work at all: the binary, the KWin grant, an icon
    for path in /usr/bin/lumine-capture /usr/share/applications/lumine-capture.desktop; do
        grep -qx "$path" <<<"$payload" || die "$path is not in the package"
    done
    grep -q '^/usr/share/icons/hicolor/.*\.png$' <<<"$payload" \
        || die "the package ships no icons"

    printf '\n\033[1;32mRPM: %s\033[0m\n' "$package"
}

readonly VERIFY='
if ldd /usr/bin/lumine-capture | grep "not found"; then
    echo "FAILED: the package does not declare all its runtime dependencies" >&2; exit 1
fi

#    KWin grants screenshot access by exact executable path,
#    so the installed Exec= and the installed binary has to be the same
desktop=/usr/share/applications/lumine-capture.desktop
exec_path=$(sed -n "s/^Exec=//p" "$desktop" | cut -d" " -f1)
[ -x "$exec_path" ] || { echo "FAILED: Exec=$exec_path is not an executable file" >&2; exit 1; }

#     again, obligatory for kde screenshotting
grep -q "^X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\$" "$desktop" \
    || { echo "FAILED: the installed .desktop has no KWin grant" >&2; exit 1; }

desktop-file-validate "$desktop"
lumine-capture --version
echo "installed and verified: $exec_path"
'

# verify_pkg IMAGE PACKAGE
verify_pkg() {
    local image=$1 package=$2 install
    case "$package" in
        *.rpm) install="dnf install -y './$package' desktop-file-utils >/dev/null" ;;
        *.deb) install="apt-get update -qq >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq './$package' desktop-file-utils >/dev/null" ;;
        *.pkg.tar.zst) install="pacman -Syu --noconfirm --needed desktop-file-utils >/dev/null && pacman -U --noconfirm './$package' >/dev/null" ;;
        *) die "don't know how to install $package" ;;
    esac
    step "install $(basename "$package") on $image"
    podman run --rm -i -v "$PWD:/src:Z" -w /src "$image" bash -euo pipefail -c "$install
$VERIFY"
}

clean() {
    have podman || die "podman is missing"

    local caches="$PWD/target/ci-cache"
    mkdir -p "$caches/cargo" "$caches/ort"
    # reuse the host's 229 MB ONNX Runtime download\
    local caches_ort="$caches/ort"
    [ -d "$HOME/.cache/ort.pyke.io" ] && caches_ort="$HOME/.cache/ort.pyke.io"

    local out="target/ci-clean/generate-rpm"
    rm -rf "$out"

    step "container A: build and package on a pristine Fedora"
    podman run --rm -i \
        -v "$PWD:/src:Z" \
        -v "$caches/cargo:/cargo:Z" \
        -v "$caches_ort:/root/.cache/ort.pyke.io:Z" \
        -e CARGO_HOME=/cargo \
        -e CARGO_TARGET_DIR=/src/target/ci-clean \
        -w /src "$IMAGE" bash -euo pipefail -c "
            dnf install -y --setopt=install_weak_deps=False \
                $BUILD_DEPS rust cargo clippy rustfmt desktop-file-utils rpm-build >/dev/null
            cargo install cargo-generate-rpm --locked --root /cargo >/dev/null
            export PATH=/cargo/bin:\$PATH
            ./scripts/ci.sh full
            ./scripts/ci.sh rpm
        "

    local package
    package=$(find "$out" -name '*.rpm' -print -quit 2>/dev/null || true)
    [ -n "$package" ] || die "container A left no RPM in $out"

    verify_pkg "$IMAGE" "$package"

    printf '\n\033[1;32mclean: the package builds and installs on a bare Fedora\033[0m\n'
}

compat() {
    have podman || die "podman is missing"

    local caches="$PWD/target/ci-cache" ort="$HOME/.cache/ort.pyke.io"
    mkdir -p "$caches/cargo-rpm" "$caches/cargo-deb" "$caches/ort"
    [ -d "$ort" ] || ort="$caches/ort"

    rm -rf target/compat-rpm/generate-rpm target/compat-deb/debian

    step "RPM: build on fedora:43"
    podman run --rm -i \
        -v "$PWD:/src:Z" -v "$caches/cargo-rpm:/cargo:Z" -v "$ort:/root/.cache/ort.pyke.io:Z" \
        -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/src/target/compat-rpm \
        -w /src registry.fedoraproject.org/fedora:43 bash -euo pipefail -c "
            # mirrors drop connections now and then, so try a few times
            for try in 1 2 3; do
                dnf install -y --setopt=install_weak_deps=False $BUILD_DEPS rust cargo rpm-build >/dev/null && break
                [ \$try = 3 ] && exit 1
            done
            cargo install cargo-generate-rpm --locked --root /cargo >/dev/null
            export PATH=/cargo/bin:\$PATH
            cargo build --profile dist --locked
            cargo generate-rpm --profile dist --target-dir /src/target/compat-rpm
        "
    local rpm
    rpm=$(find target/compat-rpm/generate-rpm -name '*.rpm' -print -quit 2>/dev/null || true)
    [ -n "$rpm" ] || die "no RPM was produced"

    step "deb: build on ubuntu:24.04 (Rust from rustup, its own is too old)"
    podman run --rm -i \
        -v "$PWD:/src:Z" -v "$caches/cargo-deb:/cargo:Z" -v "$ort:/root/.cache/ort.pyke.io:Z" \
        -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo/rustup -e CARGO_TARGET_DIR=/src/target/compat-deb \
        -w /src docker.io/library/ubuntu:24.04 bash -euo pipefail -c "
            export DEBIAN_FRONTEND=noninteractive
            apt-get update -qq >/dev/null
            apt-get install -y -qq -o Acquire::Retries=5 --no-install-recommends curl ca-certificates build-essential dpkg-dev git \
                clang libclang-dev pkg-config libpipewire-0.3-dev libspa-0.2-dev libxkbcommon-dev libssl-dev >/dev/null
            [ -x /cargo/bin/rustc ] || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path >/dev/null
            export PATH=/cargo/bin:\$PATH
            command -v cargo-deb >/dev/null || cargo install cargo-deb --locked >/dev/null
            cargo deb --profile dist --locked
        "
    local deb
    deb=$(find target/compat-deb/debian -name '*.deb' -print -quit 2>/dev/null || true)
    [ -n "$deb" ] || die "no .deb was produced in target/compat-deb/debian"

    verify_pkg registry.fedoraproject.org/fedora:43 "$rpm"
    verify_pkg registry.fedoraproject.org/fedora:44 "$rpm"
    verify_pkg docker.io/library/ubuntu:24.04 "$deb"
    verify_pkg docker.io/library/debian:13 "$deb"

    package_arch

    printf '\n\033[1;32mcompat: %s, %s and the Arch package install and start everywhere\033[0m\n' \
        "$(basename "$rpm")" "$(basename "$deb")"
}

package_arch() {
    have podman || die "podman is missing"

    local caches="$PWD/target/ci-cache" ort="$HOME/.cache/ort.pyke.io"
    mkdir -p "$caches/pacman" "$caches/ort"
    [ -d "$ort" ] || ort="$caches/ort"

    git diff --quiet HEAD || note "uncommitted changes are NOT in the Arch package (git archive HEAD)"

    local version out="$PWD/target/arch"
    version=$(sed -n 's/^pkgver=//p' packaging/arch/PKGBUILD)
    rm -rf "$out"; mkdir -p "$out"
    git archive --format=tar.gz --prefix="LumineCapture-$version/" HEAD \
        -o "$out/lumine-capture-$version.tar.gz"
    cp packaging/arch/PKGBUILD "$out/"

    step "arch: makepkg on a pristine archlinux"
    podman run --rm -i \
        -v "$out:/out:Z" -v "$caches/pacman:/var/cache/pacman/pkg:Z" -v "$ort:/ort-cache:ro,Z" \
        docker.io/library/archlinux:latest bash -euo pipefail -c "
            for try in 1 2 3; do
                pacman -Syu --noconfirm --needed base-devel >/dev/null && break
                [ \$try = 3 ] && exit 1
            done
            useradd -m builder
            # makepkg -s installs depends/makedepends from the PKGBUILD itself, so a
            # dependency missing there fails here instead of on an AUR user's machine
            echo 'builder ALL=(ALL) NOPASSWD: /usr/bin/pacman' > /etc/sudoers.d/builder
            mkdir -p /home/builder/build /home/builder/.cache
            cp /out/PKGBUILD /out/*.tar.gz /home/builder/build/
            # the build script looks for ONNX Runtime in ~/.cache/ort.pyke.io
            cp -r /ort-cache /home/builder/.cache/ort.pyke.io
            chown -R builder /home/builder
            su builder -c 'cd ~/build && makepkg -sf --noconfirm'
            cp /home/builder/build/*.pkg.tar.zst /out/
        "
    local package
    package=$(find "$out" -name '*.pkg.tar.zst' ! -name '*-debug-*' -print -quit)
    [ -n "$package" ] || die "makepkg produced no package"
    package=${package#"$PWD/"}

    verify_pkg docker.io/library/archlinux:latest "$package"
    printf '\n\033[1;32march: %s installs and starts\033[0m\n' "$(basename "$package")"
}

case "${1:-}" in
    quick)  quick ;;
    full)   full ;;
    rpm)    package_rpm ;;
    clean)  clean ;;
    compat) compat ;;
    arch)   package_arch ;;
    *)      sed -n '2,/^[^#]/{/^#/p;}' "$0" >&2; exit 2 ;;
esac

printf '\n\033[1;32mci.sh %s: ok\033[0m\n' "${1:-}"