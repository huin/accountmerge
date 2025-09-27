{ pkgs, lib, config, inputs, ... }:

{
  # https://devenv.sh/basics/
  env = {
    GREET = "devenv";
  };

  # https://devenv.sh/packages/
  packages = with pkgs; [
  ];

  # https://devenv.sh/scripts/
  scripts.hello.exec = "echo hello from $GREET";

  enterShell = ''
    hello
    git --version
  '';

  # https://devenv.sh/tests/
  # enterTest = ''
  #   echo "Running tests"
  #   git --version | grep "2.42.0"
  # '';

  # https://devenv.sh/languages/
  languages.rust.enable = true;
  languages.rust.channel = "stable";

  # https://devenv.sh/git-hooks/
  git-hooks.hooks.shellcheck.enable = true;

  # https://devenv.sh/processes/
  # processes.ping.exec = "ping example.com";

  # See full reference at https://devenv.sh/reference/options/
}
