{ pkgs, lib, config, ... }:

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
    gh            # the agent loop talks to GitHub
    jq
  ] ++ lib.optionals stdenv.isLinux appLibs;

  # the desktop app (app/)
  languages.rust.enable = true;

  env = {
    JETBRAINS_MONO_DIR = "${pkgs.jetbrains-mono}/share/fonts/truetype";
    ORBITRON_DIR = "${pkgs.orbitron}/share/fonts/truetype";
  } // lib.optionalAttrs pkgs.stdenv.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath appLibs;
  };

  # the agent loop (scripts/agent-loop.sh), started with `devenv up`. It runs
  # the `claude` CLI from your PATH.
  processes.agents.exec = "scripts/agent-loop.sh";

  # `devenv tasks run <name>`. One build.sh run per task: each one starts
  # from a fresh build/rockbox, so targets can't be separate tasks. Each task
  # writes what it made to $DEVENV_TASK_OUTPUT_FILE, printed on success.
  tasks = {
    # --input targets="ipodmini1g ipodpatcher" to pick targets
    "firmware:build" = {
      exec = ''
        set -euo pipefail
        stamp=$(mktemp)
        scripts/build.sh $(jq -r '.targets' <<<"$DEVENV_TASK_INPUT")
        find dist -type f -newer "$stamp" | sort | jq -R . | jq -s '{built: .}' >"$DEVENV_TASK_OUTPUT_FILE"
        rm "$stamp"
      '';
      input.targets = "all";
    };
    "sim:setup" = {
      exec = "scripts/sim-setup.sh build/rockbox";
      after = [ "firmware:build" ];
    };
    "sim:test" = {
      exec = ''
        set -euo pipefail
        scripts/sim-test.sh build/shots/test
        find build/shots/test -name '*.png' | wc -l | jq '{shots: "build/shots/test", screendumps: .}' >"$DEVENV_TASK_OUTPUT_FILE"
      '';
      after = [ "sim:setup" ];
    };
    "app:run" = {
      exec = "cargo run --release -p podlink";
      cwd = "${config.devenv.root}/app";
    };
    # what CI checks
    "app:test" = {
      exec = ''
        set -euo pipefail
        cargo fmt --check
        cargo clippy --all-targets --features pod-core/fixtures -- -D warnings
        cargo test 2>&1 | tee /dev/stderr \
          | awk '/^test result: ok\./ { p += $4; i += $8 } END { printf "{\"passed\": %d, \"ignored\": %d}", p, i }' \
          >"$DEVENV_TASK_OUTPUT_FILE"
      '';
      cwd = "${config.devenv.root}/app";
    };
  };
}
