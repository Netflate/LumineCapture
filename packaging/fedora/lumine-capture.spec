%global debug_package %{nil}

Name:           lumine-capture
Version:        0.3.0
Release:        1%{?dist}
Summary:        Screenshot and annotation tool for Wayland with built-in OCR

License:        (MIT OR Apache-2.0) AND MIT AND OFL-1.1
URL:            https://github.com/Netflate/LumineCapture
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz

# ONNX Runtime ships prebuilt only for these
ExclusiveArch:  x86_64 aarch64

BuildRequires:  rust >= 1.88
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  clang
BuildRequires:  pkgconf-pkg-config
BuildRequires:  pipewire-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  openssl-devel
BuildRequires:  desktop-file-utils

Requires:       hicolor-icon-theme

%description
LumineCapture is a fast, lightweight screenshot and annotation tool for
Wayland, written in Rust. It needs no background daemon, lets you annotate
the screenshot right away, recognizes text offline (PaddleOCR via ONNX
Runtime) and can pin a screenshot to the screen.

It captures through KWin, ext-image-copy-capture, wlr-screencopy or the
desktop portal, whichever the compositor offers.

%prep
%autosetup -n LumineCapture-%{version}

%build
# Fedora's RUSTFLAGS turn strip off and add debuginfo, overriding profile.dist
unset RUSTFLAGS
# the build script downloads the ONNX Runtime it links against
cargo build --locked --profile dist

%install
install -D -p -m 0755 target/dist/%{name} %{buildroot}%{_bindir}/%{name}
# KWin grants screenshot access by the Exec= path of this file, so it has to be /usr/bin/lumine-capture
install -D -p -m 0644 assets/%{name}.desktop %{buildroot}%{_datadir}/applications/%{name}.desktop
for icon in assets/hicolor/*/apps/*; do
    install -D -p -m 0644 "$icon" "%{buildroot}%{_datadir}/icons/${icon#assets/}"
done

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{name}.desktop
unset RUSTFLAGS
cargo test --locked --profile dist

%files
%license LICENSE-MIT LICENSE-APACHE assets/fonts/LICENSE-OFL.txt
%doc README.md docs/lumine-capture-shortcuts-kde.kksrc
%{_bindir}/%{name}
%{_datadir}/applications/%{name}.desktop
%{_datadir}/icons/hicolor/*/apps/lumine-capture.*
%{_datadir}/icons/hicolor/*/apps/lumine-pin.*

%changelog
* Sat Oct 17 2026 Netflate <netflate@gmail.com> - 0.3.0-1
- Update to 0.3.0

* Fri Oct 16 2026 Netflate <netflate@gmail.com> - 0.2.0-1
- Initial package
