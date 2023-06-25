{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils, naersk }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
      in
      {
        formatter = nixpkgs.legacyPackages."${system}".nixpkgs-fmt;

        defaultPackage = naersk-lib.buildPackage {
          src = ./.;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ libsodium ];
        };
        devShell = with pkgs; mkShell {
          buildInputs = [ cargo rustc rustfmt pre-commit rustPackages.clippy ];
          RUST_SRC_PATH = rustPlatform.rustLibSrc;
        };

        nixosModules.default = { config, lib, pkgs, ... }:
          with lib;
          let
            cfg = config.services.pisshoff;
          in
          {
            options.services.pisshoff = {
              enable = mkEnableOption "pisshoff";
              settings = mkOption {
                type = (pkgs.formats.toml { }).type;
                default = { };
                description = "Specify the configuration for pisshoff in Nix";
              };
            };

            config = mkIf cfg.enable {
              systemd.services.pisshoff = {
                enable = true;
                wantedBy = [ "multi-user.target" ];
                after = [ "network-online.target" ];
                serviceConfig =
                  let
                    format = pkgs.formats.toml { };
                    conf = format.generate "pisshoff.toml" cfg.settings;
                  in
                  {
                    Type = "exec";
                    ExecStart = "${self.defaultPackage."${system}"}/bin/pisshoff -c \"${conf}\"";
                    Restart = "on-failure";

                    LogsDirectory = "pisshoff";
                    CapabilityBoundingSet = "";
                    NoNewPrivileges = true;
                    PrivateDevices = true;
                    PrivateTmp = true;
                    PrivateUsers = true;
                    PrivateMounts = true;
                    ProtectHome = true;
                    ProtectClock = true;
                    ProtectProc = "invisible";
                    ProcSubset = "pid";
                    ProtectKernelLogs = true;
                    ProtectKernelModules = true;
                    ProtectKernelTunables = true;
                    ProtectControlGroups = true;
                    ProtectHostname = true;
                    ProtectSystem = "strict";
                    RestrictSUIDSGID = true;
                    RestrictRealtime = true;
                    RestrictNamespaces = true;
                    LockPersonality = true;
                    RemoveIPC = true;
                    MemoryDenyWriteExecute = true;
                    DynamicUser = true;
                    RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
                    SystemCallFilter = [ "@system-service" "~@privileged" ];
                  };
              };
            };
          };
      });
}
