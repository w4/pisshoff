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

        packages.default = naersk-lib.buildPackage {
          src = ./.;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ libsodium ];
        };

        devShells.default = with pkgs; mkShell {
          buildInputs = [ cargo rustc rustfmt pre-commit rustPackages.clippy ];
          RUST_SRC_PATH = rustPlatform.rustLibSrc;
        };

        nixosModules.pisshoff-server = { config, lib, pkgs, ... }:
          with lib;
          let
            cfg = config.services.pisshoff-server;
          in
          {
            options.services.pisshoff-server = {
              enable = mkEnableOption "pisshoff-server";
              settings = mkOption {
                type = (pkgs.formats.toml { }).type;
                default = { };
                description = "Specify the configuration for pisshoff-server in Nix";
              };
            };

            config = mkIf cfg.enable {
              systemd.services.pisshoff-server = {
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
                    ExecStart = "${self.packages."${system}".default}/bin/pisshoff-server -c \"${conf}\"";
                    ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
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

              services.logrotate.settings.pisshoff-server = {
                files = "/var/log/pisshoff/audit.log";
                rotate = 31;
                frequency = "daily";
                compress = true;
                delaycompress = true;
                missingok = true;
                notifempty = true;
                postrotate = "systemctl reload pisshoff-server";
              };
            };
          };

        nixosModules.pisshoff-timescaledb-exporter = { config, lib, pkgs, ... }:
          with lib;
          let
            cfg = config.services.pisshoff-timescaledb-exporter;
          in
          {
            options.services.pisshoff-timescaledb-exporter = {
              enable = mkEnableOption "pisshoff-timescaledb-exporter";
              settings = mkOption {
                type = (pkgs.formats.toml { }).type;
                default = { };
                description = "Specify the configuration for pisshoff-timescaledb-exporter in Nix";
              };
            };

            config = mkIf cfg.enable {
              systemd.services.pisshoff-timescaledb-exporter = {
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
                    ExecStart = "${self.packages."${system}".default}/bin/pisshoff-timescaledb-exporter -c \"${conf}\"";
                    ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
                    Restart = "on-failure";

                    RuntimeDirectory = "pisshoff-timescaledb-exporter";
                    LogsDirectory = "pisshoff-timescaledb-exporter";
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
                    RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
                    SystemCallFilter = [ "@system-service" "~@privileged" ];
                  };
              };
            };
          };
      });
}
