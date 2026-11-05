{ pkgs, lib, ... }:

let
  # Rockbox's configure looks for an `arm-elf-eabi-` prefixed toolchain.
  # Expose nixpkgs' arm-none-eabi GCC under that prefix.
  armPrefixShim = pkgs.runCommand "arm-elf-eabi-shim" { } ''
    mkdir -p $out/bin
    for t in ${pkgs.gcc-arm-embedded}/bin/arm-none-eabi-*; do
      name=$(basename "$t")
      ln -s "$t" "$out/bin/arm-elf-eabi-''${name#arm-none-eabi-}"
    done
  '';
  # Rockbox builds its host tools with a hardcoded `gcc`; point it at clang.
  hostGccShim = pkgs.writeShellScriptBin "gcc" ''exec ${pkgs.stdenv.cc}/bin/cc "$@"'';
  # Linux runtime libraries for the app's window (winit, wgpu)
  appLibs = with pkgs; [
    wayland
    libxkbcommon
    vulkan-loader
    libGL
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
  ];
  # On macOS, the simulator's configure insists on Homebrew-style `gcc-16`.
  simGccShim = pkgs.runCommand "sim-gcc16-shim" { } ''
    mkdir -p $out/bin
    ln -s ${pkgs.gcc16}/bin/gcc $out/bin/gcc-16
    ln -s ${pkgs.gcc16}/bin/ar $out/bin/gcc-ar-16
    ln -s ${pkgs.gcc16}/bin/cpp $out/bin/cpp-16
  '';
in
{
  packages = with pkgs; [
    gcc-arm-embedded
    armPrefixShim
    hostGccShim
    simGccShim
    gnumake
    perl
    SDL2          # UI simulator
    pkg-config
    imagemagick   # theme asset generation
    python3
    zip
    flac          # test library for the simulator
    jetbrains-mono
    orbitron
    otf2bdf       # rasterize brand fonts to BDF
  ] ++ lib.optionals stdenv.isLinux appLibs;

  # the desktop app (app/)
  languages.rust.enable = true;

  env = {
    JETBRAINS_MONO_DIR = "${pkgs.jetbrains-mono}/share/fonts/truetype";
    ORBITRON_DIR = "${pkgs.orbitron}/share/fonts/truetype";
  } // lib.optionalAttrs pkgs.stdenv.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath appLibs;
  };
}
