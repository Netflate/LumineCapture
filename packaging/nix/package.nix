{
  lib,
  stdenv,
  fetchurl,
  dpkg,
  autoPatchelfHook,
  openssl,
  libxkbcommon,
  pipewire,
  wayland,
  libGL,
  vulkan-loader,
}:

let
  pname = "lumine-capture";
  version = "0.3.0";

  src = fetchurl {
    url = "https://github.com/Netflate/LumineCapture/releases/download/v${version}/lumine-capture_${version}-1_amd64.deb";
    hash = "sha256-w97zkBbBquCn2Oetrix2arFs+YYcAbeUzvg/AzGE5Yg=";
  };
in
stdenv.mkDerivation {
  inherit pname version src;

  nativeBuildInputs = [
    dpkg
    autoPatchelfHook
  ];

  # Linked directly by the binary (libstdc++/libgcc_s come from cc.lib)
  buildInputs = [
    stdenv.cc.cc.lib
    openssl
    libxkbcommon
    pipewire
  ];

  # Not in DT_NEEDED, but the GUI/GPU stack may dlopen these at runtime
  runtimeDependencies = [
    wayland
    libGL
    vulkan-loader
    libxkbcommon
  ];

  unpackPhase = ''
    runHook preUnpack
    dpkg-deb -x $src .
    runHook postUnpack
  '';

  installPhase = ''
    runHook preInstall

    install -Dm755 usr/bin/lumine-capture $out/bin/lumine-capture

    install -Dm644 usr/share/applications/lumine-capture.desktop \
      $out/share/applications/lumine-capture.desktop
    substituteInPlace $out/share/applications/lumine-capture.desktop \
      --replace-fail 'Name=LumineCapture' 'Name=Lumine Capture' \
      --replace-fail 'Exec=/usr/bin/lumine-capture' "Exec=$out/bin/lumine-capture"

    cp -r usr/share/icons $out/share/

    install -Dm644 usr/share/doc/lumine-capture/LICENSE-MIT \
      usr/share/doc/lumine-capture/LICENSE-APACHE \
      usr/share/doc/lumine-capture/LICENSE-OFL-Inter.txt \
      -t $out/share/licenses/lumine-capture
    install -Dm644 usr/share/doc/lumine-capture/lumine-capture-shortcuts-kde.kksrc \
      -t $out/share/doc/lumine-capture

    runHook postInstall
  '';

  meta = {
    description = "Fast, lightweight Wayland screenshot and annotation tool with built-in OCR";
    homepage = "https://github.com/Netflate/LumineCapture";
    license = with lib.licenses; [
      mit
      asl20
      ofl
    ];
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    platforms = [ "x86_64-linux" ];
    mainProgram = "lumine-capture";
  };
}
