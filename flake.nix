{
  description = "LumineCapture: fast, lightweight Wayland screenshot and annotation tool with built-in OCR";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      packages.${system} = {
        lumine-capture = pkgs.callPackage ./packaging/nix/package.nix { };
        default = self.packages.${system}.lumine-capture;
      };

      overlays.default = final: _prev: {
        lumine-capture = final.callPackage ./packaging/nix/package.nix { };
      };
    };
}
