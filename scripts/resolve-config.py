#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import sys
import tomllib
from pathlib import Path
from typing import Any


class ConfigError(Exception):
    pass


def err(msg: str) -> None:
    raise ConfigError(msg)


def as_dict(value: Any, ctx: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        err(f"{ctx} must be a table/object")
    return value


def as_list(value: Any, ctx: str) -> list[Any]:
    if value is None:
        return []
    if not isinstance(value, list):
        err(f"{ctx} must be an array")
    return value


def as_str(value: Any, ctx: str) -> str:
    if not isinstance(value, str) or value.strip() == "":
        err(f"{ctx} must be a non-empty string")
    return value


def as_bool(value: Any, ctx: str, default: bool = False) -> bool:
    if value is None:
        return default
    if not isinstance(value, bool):
        err(f"{ctx} must be a boolean")
    return value


def as_int(value: Any, ctx: str) -> int:
    if not isinstance(value, int):
        err(f"{ctx} must be an integer")
    return value


def expand_path(path: str) -> str:
    expanded = os.path.expandvars(os.path.expanduser(path))
    return str(Path(expanded).resolve())


def norm_mode(mode: str, ctx: str) -> str:
    mode = as_str(mode, ctx).lower()
    if mode not in {"ro", "rw"}:
        err(f"{ctx} must be 'ro' or 'rw'")
    return mode


def norm_when(when: str, ctx: str) -> str:
    when = as_str(when, ctx).lower()
    if when not in {"always", "browser"}:
        err(f"{ctx} must be 'always' or 'browser'")
    return when


def normalize_mount(entry: dict[str, Any], ctx: str) -> dict[str, Any]:
    host = expand_path(as_str(entry.get("host"), f"{ctx}.host"))
    container = as_str(entry.get("container"), f"{ctx}.container")
    mode = norm_mode(entry.get("mode"), f"{ctx}.mode")
    when = norm_when(entry.get("when", "always"), f"{ctx}.when")
    kind = as_str(entry.get("kind", "dir"), f"{ctx}.kind").lower()
    if kind not in {"dir", "file"}:
        err(f"{ctx}.kind must be 'dir' or 'file'")

    return {
        "host": host,
        "container": container,
        "mode": mode,
        "when": when,
        "kind": kind,
        "create": as_bool(entry.get("create"), f"{ctx}.create", default=False),
        "optional": as_bool(entry.get("optional"), f"{ctx}.optional", default=False),
        "source": as_str(entry.get("source", "config"), f"{ctx}.source"),
    }


def normalize_secret(entry: dict[str, Any], ctx: str) -> list[dict[str, Any]]:
    env_name = as_str(entry.get("env"), f"{ctx}.env")
    out: list[dict[str, Any]] = []

    from_env = entry.get("from_env")
    if from_env is not None:
        out.append(
            {
                "env": env_name,
                "source": "env",
                "from_env": as_str(from_env, f"{ctx}.from_env"),
                "origin": ctx,
            }
        )

    secret_store = entry.get("secret_store")
    if secret_store is not None:
        attrs = as_dict(secret_store, f"{ctx}.secret_store")
        normalized_attrs: dict[str, str] = {}
        for key, value in attrs.items():
            normalized_attrs[as_str(key, f"{ctx}.secret_store key")] = as_str(value, f"{ctx}.secret_store.{key}")
        if not normalized_attrs:
            err(f"{ctx}.secret_store must include at least one lookup attribute")

        out.append(
            {
                "env": env_name,
                "source": "secret-tool",
                "attributes": normalized_attrs,
                "origin": ctx,
            }
        )

    # legacy explicit provider form (still supported)
    provider = entry.get("provider")
    if provider is not None:
        provider = as_str(provider, f"{ctx}.provider").lower()
        if provider == "env":
            source_var = as_str(entry.get("var", env_name), f"{ctx}.var")
            out.append({"env": env_name, "source": "env", "from_env": source_var, "origin": ctx})
        elif provider == "secret-tool":
            attrs = as_dict(entry.get("attributes"), f"{ctx}.attributes")
            normalized_attrs = {as_str(k, f"{ctx}.attributes key"): as_str(v, f"{ctx}.attributes.{k}") for k, v in attrs.items()}
            if not normalized_attrs:
                err(f"{ctx}.attributes must include at least one lookup attribute")
            out.append({"env": env_name, "source": "secret-tool", "attributes": normalized_attrs, "origin": ctx})
        else:
            err(f"{ctx}.provider must be 'env' or 'secret-tool'")

    if not out:
        err(f"{ctx} must define at least one source: from_env, secret_store, or provider")

    return out


def normalize_config(raw: dict[str, Any], config_path: str) -> dict[str, Any]:
    sandbox = as_dict(raw.get("sandbox"), "[sandbox]")

    normalized_sandbox: dict[str, Any] = {
        "image": as_str(sandbox.get("image"), "[sandbox].image"),
        "containerfile": expand_path(as_str(sandbox.get("containerfile"), "[sandbox].containerfile")),
        "sandbox_pi_dir": expand_path(as_str(sandbox.get("sandbox_pi_dir"), "[sandbox].sandbox_pi_dir")),
        "host_pi_dir": expand_path(as_str(sandbox.get("host_pi_dir"), "[sandbox].host_pi_dir")),
        "host_claude_dir": expand_path(as_str(sandbox.get("host_claude_dir"), "[sandbox].host_claude_dir")),
        "cache_dir": expand_path(as_str(sandbox.get("cache_dir"), "[sandbox].cache_dir")),
        "gitconfig_path": expand_path(as_str(sandbox.get("gitconfig_path"), "[sandbox].gitconfig_path")),
        "auth_key": expand_path(as_str(sandbox.get("auth_key"), "[sandbox].auth_key")),
        "sign_key": expand_path(as_str(sandbox.get("sign_key"), "[sandbox].sign_key")),
    }

    normalized_sandbox["bootstrap_files"] = [
        as_str(v, "[sandbox].bootstrap_files[]") for v in as_list(sandbox.get("bootstrap_files", []), "[sandbox].bootstrap_files")
    ]
    normalized_sandbox["passthrough_env"] = [
        as_str(v, "[sandbox].passthrough_env[]") for v in as_list(sandbox.get("passthrough_env", []), "[sandbox].passthrough_env")
    ]
    normalized_sandbox["container_boot_dirs"] = [
        as_str(v, "[sandbox].container_boot_dirs[]")
        for v in as_list(sandbox.get("container_boot_dirs", []), "[sandbox].container_boot_dirs")
    ]

    mounts: list[dict[str, Any]] = []
    for idx, mount in enumerate(as_list(raw.get("mount", []), "[[mount]]")):
        mounts.append(normalize_mount(as_dict(mount, f"[[mount]] #{idx}"), f"[[mount]] #{idx}"))

    secrets: list[dict[str, Any]] = []
    for idx, secret in enumerate(as_list(raw.get("secret", []), "[[secret]]")):
        secrets.extend(normalize_secret(as_dict(secret, f"[[secret]] #{idx}"), f"[[secret]] #{idx}"))

    tools: list[dict[str, Any]] = []
    for idx, tool in enumerate(as_list(raw.get("tool", []), "[[tool]]")):
        tctx = f"[[tool]] #{idx}"
        tool_obj = as_dict(tool, tctx)

        name = as_str(tool_obj.get("name"), f"{tctx}.name")
        path = expand_path(as_str(tool_obj.get("path"), f"{tctx}.path"))
        container_path = as_str(tool_obj.get("container_path"), f"{tctx}.container_path")
        mode = norm_mode(tool_obj.get("mode", "ro"), f"{tctx}.mode")
        when = norm_when(tool_obj.get("when", "always"), f"{tctx}.when")
        optional = as_bool(tool_obj.get("optional"), f"{tctx}.optional", default=False)

        tools.append(
            {
                "name": name,
                "path": path,
                "container_path": container_path,
                "mode": mode,
                "when": when,
                "optional": optional,
            }
        )

        mounts.append(
            {
                "host": path,
                "container": container_path,
                "mode": mode,
                "when": when,
                "kind": "file",
                "create": False,
                "optional": optional,
                "source": f"tool:{name}:binary",
            }
        )

        for didx, directory in enumerate(as_list(tool_obj.get("directory", []), f"{tctx}.directory")):
            mctx = f"{tctx}.directory[{didx}]"
            m = normalize_mount(as_dict(directory, mctx), mctx)
            m["source"] = f"tool:{name}:directory"
            mounts.append(m)

        for sidx, secret in enumerate(as_list(tool_obj.get("secret", []), f"{tctx}.secret")):
            sctx = f"{tctx}.secret[{sidx}]"
            for entry in normalize_secret(as_dict(secret, sctx), sctx):
                entry["tool"] = name
                secrets.append(entry)

    browser_raw = as_dict(raw.get("browser", {}), "[browser]")
    browser_enabled = as_bool(browser_raw.get("enabled"), "[browser].enabled", default=False)

    browser: dict[str, Any] = {"enabled": browser_enabled}
    if browser_enabled:
        command = as_str(browser_raw.get("command"), "[browser].command")
        if "/" in command or command.startswith("~"):
            command = expand_path(command)

        browser["command"] = command
        browser["profile_dir"] = expand_path(as_str(browser_raw.get("profile_dir"), "[browser].profile_dir"))
        browser["debug_port"] = as_int(browser_raw.get("debug_port"), "[browser].debug_port")
        browser["pi_skill_path"] = as_str(browser_raw.get("pi_skill_path", ""), "[browser].pi_skill_path") if browser_raw.get("pi_skill_path") else ""
        browser["command_args"] = [
            as_str(v, "[browser].command_args[]") for v in as_list(browser_raw.get("command_args", []), "[browser].command_args")
        ]
    else:
        browser.update({"command": "", "profile_dir": "", "debug_port": 0, "pi_skill_path": "", "command_args": []})

    update_raw = as_dict(raw.get("update", {}), "[update]")
    update = {
        "pi_spec": as_str(update_raw.get("pi_spec", "@mariozechner/pi-coding-agent"), "[update].pi_spec"),
        "minimum_release_age": as_int(update_raw.get("minimum_release_age", 1440), "[update].minimum_release_age"),
    }

    return {
        "config_file": config_path,
        "sandbox": normalized_sandbox,
        "mounts": mounts,
        "tools": tools,
        "secrets": secrets,
        "browser": browser,
        "update": update,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Resolve and validate pi-sandbox config TOML")
    parser.add_argument("--config", required=True, help="Path to config TOML")
    args = parser.parse_args()

    config_file = expand_path(args.config)
    if not Path(config_file).is_file():
        print(f"Config file not found: {config_file}", file=sys.stderr)
        return 2

    try:
        with open(config_file, "rb") as f:
            raw = tomllib.load(f)
        resolved = normalize_config(raw, config_file)
    except tomllib.TOMLDecodeError as e:
        print(f"Invalid TOML in {config_file}: {e}", file=sys.stderr)
        return 2
    except ConfigError as e:
        print(f"Invalid config {config_file}: {e}", file=sys.stderr)
        return 2

    json.dump(resolved, sys.stdout, sort_keys=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
