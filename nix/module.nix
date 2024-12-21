{ config
, lib
, pkgs
, ...
}:
with lib; let
  cfg = config.services.dispenser;
  format = pkgs.formats.toml { };
  configFile = format.generate "dispenser.toml" (filterAttrs (n: v: v != null) {
    inherit (cfg) server vultr dyndns schedule;
    digital_ocean = cfg.digitalocean;
  });
in
{
  options.services.dispenser = {
    enable = mkEnableOption "Enables the dispenser service";

    server = mkOption {
      type = types.submodule {
        options = {
          rcon = mkOption {
            type = types.str;
            description = "Rcon password for created server";
          };
          password = mkOption {
            type = types.str;
            description = "Server password for created server";
          };
          demostf_key = mkOption {
            type = types.str;
            description = "Api key for demos.tf";
          };
          logstf_key = mkOption {
            type = types.str;
            description = "Api key for logs.tf";
          };
          config_league = mkOption {
            type = types.str;
            default = "etf2l";
            description = "League of the config to load on startup";
          };
          config_mode = mkOption {
            type = types.str;
            default = "6v6";
            description = "Gamemode of the config to load on startup";
          };
          name = mkOption {
            type = types.str;
            default = "Spire";
            description = "Server name for the created server";
          };
          tv_name = mkOption {
            type = types.str;
            default = "SpireTV";
            description = "STV name for the created server";
          };
          image = mkOption {
            type = types.str;
            default = "spiretf/docker-spire-server";
            description = "Docker image to use for the server";
          };
          ssh_keys = mkOption {
            type = types.listOf types.str;
            description = "ssh keys to allow on the server";
          };
          manage_existing = mkOption {
            type = types.bool;
            description = "Take control of existing server";
          };
          extra_cfg = mkOption {
            type = types.str;
            description = "Extra config lines to set in the tf2 config";
            default = "";
          };
        };
      };
    };

    vultr = mkOption {
      type = types.nullOr (types.submodule {
        options = {
          api_key = mkOption {
            type = types.str;
            description = "Vultr api key";
          };
          region = mkOption {
            type = types.str;
            default = "ams";
            description = "Vultr region to deploy the server in";
          };
          plan = mkOption {
            type = types.str;
            default = "vc2-1c-2gb";
            description = "Vultr plan to deploy";
          };
        };
      });
      default = null;
    };

    digitalocean = mkOption {
      type = types.nullOr (types.submodule {
        options = {
          api_key = mkOption {
            type = types.str;
            description = "DO api key";
          };
          region = mkOption {
            type = types.str;
            default = "ams3";
            description = "DO region to deploy the server in";
          };
          plan = mkOption {
            type = types.str;
            default = "s-1vcpu-2gb";
            description = "DO plan to deploy";
          };
        };
      });
      default = null;
    };

    dyndns = mkOption {
      type = types.nullOr (types.submodule {
        options = {
          update_url = mkOption {
            type = types.str;
            description = "dyndns update url";
          };
          hostname = mkOption {
            type = types.str;
            description = "hostname to update";
          };
          username = mkOption {
            type = types.str;
            description = "username for the update";
          };
          password = mkOption {
            type = types.str;
            description = "password for the update";
          };
        };
      });
      default = null;
    };

    schedule = mkOption {
      type = types.submodule {
        options = {
          start = mkOption {
            type = types.str;
            description = "start schedule in cron format";
          };
          stop = mkOption {
            type = types.str;
            description = "start schedule in cron format";
          };
        };
      };
    };

    package = mkOption {
      type = types.package;
      defaultText = literalExpression "pkgs.dispenser";
      description = "package to use";
    };
  };

  config = mkIf cfg.enable {
    systemd.services.dispenser = {
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/dispenser ${configFile}";
        Restart = "on-failure";
        DynamicUser = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        ProtectClock = true;
        CapabilityBoundingSet = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        SystemCallArchitectures = "native";
        ProtectKernelModules = true;
        RestrictNamespaces = true;
        MemoryDenyWriteExecute = true;
        ProtectHostname = true;
        LockPersonality = true;
        ProtectKernelTunables = true;
        RestrictAddressFamilies = "AF_INET AF_INET6";
        RestrictRealtime = true;
        ProtectProc = "noaccess";
        SystemCallFilter = [ "@system-service" "~@resources" "~@privileged" ];
        IPAddressDeny = "localhost link-local multicast";
      };
    };

    environment.systemPackages = [
      (pkgs.writeShellApplication {
        name = "dispenser-cli";
        runtimeInputs = [ cfg.package ];

        text = ''
          ${cfg.package}/bin/dispenser ${configFile} "$@"
        '';
      })
    ];
  };
}
