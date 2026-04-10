#!/usr/bin/env python3
"""Build Tandem marketplace pack zip packages from source trees.

The marketplace deliverable is the zip package. The source tree can live
anywhere; the builder accepts an explicit packs root or explicit pack
directories so we do not need to publish the authoring layer in git.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Iterable, List
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

try:
    import yaml  # type: ignore
except Exception as exc:  # pragma: no cover - dependency error path
    raise SystemExit(
        "This script requires PyYAML (`python3 -m pip install pyyaml`)."
    ) from exc


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PACKS_ROOT = REPO_ROOT / "docs" / "internal" / "marketplace" / "packs"
DEFAULT_OUTPUT_ROOT = REPO_ROOT / "docs" / "internal" / "marketplace" / "dist"


def load_manifest(pack_dir: Path) -> Dict[str, Any]:
    manifest_path = pack_dir / "tandempack.yaml"
    if not manifest_path.exists():
        raise SystemExit(f"missing manifest: {manifest_path}")
    with manifest_path.open("r", encoding="utf-8") as handle:
        manifest = yaml.safe_load(handle) or {}
    if not isinstance(manifest, dict):
        raise SystemExit(f"manifest must be a mapping: {manifest_path}")
    return manifest


def require_text(value: Any, label: str, pack_dir: Path) -> str:
    text = str(value or "").strip()
    if not text:
        raise SystemExit(f"{label} is required for {pack_dir}")
    return text


def as_string_list(value: Any) -> List[str]:
    if not isinstance(value, list):
        return []
    out: List[str] = []
    for item in value:
        text = str(item or "").strip()
        if text:
            out.append(text)
    return out


def marketplace_block(manifest: Dict[str, Any]) -> Dict[str, Any]:
    block = manifest.get("marketplace") or {}
    if not isinstance(block, dict):
        return {}
    return block


def declared_paths(manifest: Dict[str, Any]) -> List[str]:
    paths: List[str] = []
    contents = manifest.get("contents") or {}
    if isinstance(contents, dict):
        for _, rows in contents.items():
            if not isinstance(rows, list):
                continue
            for row in rows:
                if isinstance(row, dict):
                    path = str(row.get("path") or "").strip()
                    if path:
                        paths.append(path)

    marketplace = marketplace_block(manifest)
    listing = marketplace.get("listing") or {}
    if isinstance(listing, dict):
        icon = str(listing.get("icon") or "").strip()
        if icon:
            paths.append(icon)
        changelog = str(listing.get("changelog") or "").strip()
        if changelog:
            paths.append(changelog)
        for shot in as_string_list(listing.get("screenshots")):
            paths.append(shot)
    return sorted(dict.fromkeys(paths))


def validate_manifest(pack_dir: Path, manifest: Dict[str, Any]) -> None:
    require_text(manifest.get("manifest_schema_version"), "manifest_schema_version", pack_dir)
    require_text(manifest.get("pack_id"), "pack_id", pack_dir)
    require_text(manifest.get("name"), "name", pack_dir)
    require_text(manifest.get("version"), "version", pack_dir)
    require_text(manifest.get("type"), "type", pack_dir)

    engine = manifest.get("engine") or {}
    if not isinstance(engine, dict):
        raise SystemExit(f"engine must be a mapping in {pack_dir}")
    require_text(engine.get("requires"), "engine.requires", pack_dir)

    marketplace = marketplace_block(manifest)
    if marketplace:
        publisher = marketplace.get("publisher") or {}
        listing = marketplace.get("listing") or {}
        if not isinstance(publisher, dict) or not isinstance(listing, dict):
            raise SystemExit(f"marketplace.publisher/listing must be mappings in {pack_dir}")

        for key in ("publisher_id", "display_name", "verification_tier"):
            require_text(publisher.get(key), f"marketplace.publisher.{key}", pack_dir)
        for key in ("display_name", "description", "license_spdx"):
            require_text(listing.get(key), f"marketplace.listing.{key}", pack_dir)
        if not as_string_list(listing.get("categories")):
            raise SystemExit(f"marketplace.listing.categories is required in {pack_dir}")
        if not as_string_list(listing.get("tags")):
            raise SystemExit(f"marketplace.listing.tags is required in {pack_dir}")

    for rel in declared_paths(manifest):
        abs_path = pack_dir / rel
        if not abs_path.exists():
            raise SystemExit(f"declared pack file missing: {abs_path}")


def iter_pack_dirs(packs_root: Path) -> Iterable[Path]:
    if not packs_root.exists():
        raise SystemExit(f"missing packs root: {packs_root}")
    for entry in sorted(packs_root.iterdir()):
        if entry.is_dir():
            yield entry


def zip_dir(src_dir: Path, zip_path: Path) -> None:
    with ZipFile(zip_path, "w", compression=ZIP_DEFLATED) as archive:
        for file_path in sorted(p for p in src_dir.rglob("*") if p.is_file()):
            rel = file_path.relative_to(src_dir).as_posix()
            info = ZipInfo(rel)
            info.compress_type = ZIP_DEFLATED
            info.external_attr = (0o644 & 0xFFFF) << 16
            with file_path.open("rb") as source:
                archive.writestr(info, source.read())


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_listing(pack_dir: Path, manifest: Dict[str, Any], zip_path: Path) -> Dict[str, Any]:
    marketplace = marketplace_block(manifest)
    publisher = marketplace.get("publisher") or {}
    listing = marketplace.get("listing") or {}
    pack_id = str(manifest.get("pack_id")).strip()
    name = str(manifest.get("name")).strip()
    version = str(manifest.get("version")).strip()
    sha256 = sha256_file(zip_path)
    size_bytes = zip_path.stat().st_size
    try:
        source_dir = pack_dir.relative_to(REPO_ROOT).as_posix()
    except ValueError:
        source_dir = pack_dir.as_posix()
    return {
        "schema_version": "1",
        "pack_id": pack_id,
        "name": name,
        "version": version,
        "publisher": {
            "publisher_id": str(publisher.get("publisher_id") or "").strip(),
            "display_name": str(publisher.get("display_name") or "").strip(),
            "verification_tier": str(publisher.get("verification_tier") or "unverified").strip(),
            "website": str(publisher.get("website") or "").strip() or None,
            "support": str(publisher.get("support") or "").strip() or None,
        },
        "listing": {
            "display_name": str(listing.get("display_name") or "").strip(),
            "description": str(listing.get("description") or "").strip(),
            "categories": as_string_list(listing.get("categories")),
            "tags": as_string_list(listing.get("tags")),
            "license_spdx": str(listing.get("license_spdx") or "").strip(),
            "icon_url": str(listing.get("icon") or "").strip() or None,
            "screenshot_urls": as_string_list(listing.get("screenshots")),
            "changelog_url": str(listing.get("changelog") or "").strip() or None,
        },
        "distribution": {
            "download_url": zip_path.name,
            "sha256": sha256,
            "size_bytes": size_bytes,
            "signature_status": "missing",
        },
        "pack_source_dir": source_dir,
        "workflow_ids": as_string_list((manifest.get("entrypoints") or {}).get("workflows"))
        if isinstance(manifest.get("entrypoints"), dict)
        else [],
        "capabilities": manifest.get("capabilities") or {},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--packs-root",
        default=str(DEFAULT_PACKS_ROOT),
        help="Directory that contains the pack source trees.",
    )
    parser.add_argument(
        "--output-root",
        default=str(DEFAULT_OUTPUT_ROOT),
        help="Directory where built zips and catalog.json should be written.",
    )
    parser.add_argument(
        "pack_dirs",
        nargs="*",
        help="Optional explicit pack directories to build instead of scanning packs-root.",
    )
    args = parser.parse_args()

    packs_root = Path(args.packs_root).resolve()
    output_root = Path(args.output_root).resolve()
    output_root.mkdir(parents=True, exist_ok=True)

    if args.pack_dirs:
        pack_dirs = [Path(item).resolve() for item in args.pack_dirs]
    else:
        pack_dirs = list(iter_pack_dirs(packs_root))

    catalog: List[Dict[str, Any]] = []
    for pack_dir in pack_dirs:
        manifest = load_manifest(pack_dir)
        validate_manifest(pack_dir, manifest)
        name = require_text(manifest.get("name"), "name", pack_dir)
        version = require_text(manifest.get("version"), "version", pack_dir)
        zip_path = output_root / f"{name}-{version}.zip"
        zip_dir(pack_dir, zip_path)
        catalog.append(build_listing(pack_dir, manifest, zip_path))
        print(f"built {zip_path}")

    catalog_path = output_root / "catalog.json"
    with catalog_path.open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "schema_version": "1",
                "generated_at": datetime.now(timezone.utc).isoformat(),
                "packs": catalog,
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")
    print(f"wrote {catalog_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
