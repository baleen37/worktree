{ bash, coreutils, fish, gitMinimal, lib, rustPlatform, systems, zsh }:

let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  remapBuildPath = ''
    export RUSTFLAGS="''${RUSTFLAGS:+$RUSTFLAGS }--remap-path-prefix=$NIX_BUILD_TOP=/build"
  '';
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  version = cargoToml.package.version;
  src = ../.;
  cargoLock.lockFile = ../Cargo.lock;
  nativeCheckInputs = [ bash coreutils fish gitMinimal zsh ];

  preBuild = remapBuildPath;

  preCheck = ''
    ${remapBuildPath}
    # Nix-Darwin's global zshenv otherwise replaces PATH in shell integration tests.
    export __NIX_DARWIN_SET_ENVIRONMENT_DONE=1
  '';

  meta = {
    description = cargoToml.package.description;
    homepage = cargoToml.package.homepage;
    license = with lib.licenses; [ mit asl20 ];
    mainProgram = "wt";
    platforms = systems;
  };
}
