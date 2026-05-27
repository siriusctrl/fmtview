#!/usr/bin/env python3
"""Generate the long inline JSONL viewer fixture."""

import json
from pathlib import Path


OUT = Path(__file__).with_name("long-inline.jsonl")


def inline_text(record: int) -> str:
    parts = []
    for segment in range(1, 49):
        parts.append(
            "record {record:02d} segment {segment:03d} keeps one JSON string "
            "long enough to wrap through a terminal viewport while staying "
            "inside one logical value; structure jump should keep treating "
            "the surrounding object as not fully observed until the wrapped "
            "inline value has really been read.".format(
                record=record,
                segment=segment,
            )
        )
    return " ".join(parts)


def short_record(record: int) -> dict:
    return {
        "id": record,
        "kind": "short",
        "payload": {
            "summary": f"short record {record} before or after a long inline case",
            "flags": ["visible", "small", "skip-candidate"],
        },
        "message": "This record should usually fit in one viewport and be easy to skip.",
    }


def medium_record(record: int) -> dict:
    return {
        "id": record,
        "kind": "medium",
        "payload": {
            "route": ["ingest", "normalize", "preview"],
            "body": {
                "items": [
                    {"name": "alpha", "state": "queued"},
                    {"name": "beta", "state": "ready"},
                    {"name": "gamma", "state": "done"},
                ],
                "summary": (
                    "Medium record with enough nested structure to test block "
                    "jumps between visible objects and arrays."
                ),
            },
        },
        "message": "Medium record for structure jump spacing.",
    }


def long_record(record: int) -> dict:
    return {
        "id": record,
        "kind": "long-inline",
        "payload": {
            "route": ["ingest", "normalize", "render"],
            "body": {
                "summary": (
                    "The inline field is intentionally one very long JSON string "
                    "so wrap mode fills multiple terminal viewports without adding "
                    "more source lines."
                ),
                "inline": inline_text(record),
            },
            "limits": {
                "maxDepth": 4,
                "expected": "wrap fills the viewport",
            },
        },
        "message": "Long inline JSONL record for structure jump testing.",
    }


records = []
for group in range(4):
    base = group * 4 + 1
    records.extend(
        [
            short_record(base),
            long_record(base + 1),
            medium_record(base + 2),
            short_record(base + 3),
        ]
    )

OUT.write_text(
    "".join(json.dumps(record, separators=(",", ":")) + "\n" for record in records),
    encoding="utf-8",
)
print(f"wrote {OUT} ({len(records)} records, {OUT.stat().st_size} bytes)")
