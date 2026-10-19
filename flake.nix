{
  description = "A Git worktree manager written in Rust";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      mkWorktree = pkgs: pkgs.callPackage ./nix/package.nix { inherit systems; };
    in
    {
      overlays.default = final: _prev: { worktree = mkWorktree final; };

      packages = forAllSystems (system: {
        default = mkWorktree nixpkgs.legacyPackages.${system};
      });
    };
}
