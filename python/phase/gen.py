# simplified version of crates/engine/src/bin/oracle_gen.rs
import argparse
import gzip
import json
import os
import urllib.request
from collections import defaultdict

from phase import build_oracle_face, build_oracle_face_multi

INPUT_PATH = "AtomicCards.json.gz"
OUTPUT_PATH = "card-data.json"
SOURCE_URL = "https://mtgjson.com/api/v5/AtomicCards.json.gz"
SET_SOURCE_URL = "https://mtgjson.com/api/v5/{set_name}.json.gz"

MULTI_LAYOUTS = {
    "split",
    "flip",
    "transform",
    "meld",
    "adventure",
    "modal_dfc",
    "prepare",
    "aftermath",
}


def legality_score(card: dict) -> int:
    return sum(1 for status in (card.get("legalities") or {}).values() if status.lower() == "legal")


def make_entry(face: dict, layout=None, face_index=None) -> dict:
    entry = dict(face)
    if layout:
        entry["layout"] = layout
    if face_index is not None:
        entry["face_index"] = face_index
    return entry


def insert_face(result: dict, key: str, entry: dict) -> None:
    existing = result.get(key)
    if existing is None:
        result[key] = entry
        return
    # Standalone paper cards win over a multi-face back face of the same name.
    if existing.get("layout") and not entry.get("layout"):
        result[key] = entry


def oracle_id(card: dict) -> str | None:
    return (card.get("identifiers") or {}).get("scryfallOracleId")


def load_json(path: str):
    if path.endswith(".gz"):
        with gzip.open(path, "rt", encoding="utf-8") as f:
            return json.load(f)
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def dedupe_set_faces(cards: list) -> list:
    unique = []
    seen = set()
    for card in cards:
        key = (card.get("faceName") or card.get("name"), card.get("side"), oracle_id(card))
        if key in seen:
            continue
        seen.add(key)
        unique.append(card)
    unique.sort(key=lambda c: c.get("side") or "a")
    return unique


def groups_from_set_cards(cards: list):
    by_name = defaultdict(list)
    for card in cards:
        if isinstance(card, dict) and card.get("name"):
            by_name[card["name"]].append(card)
    for name, group in by_name.items():
        yield name, dedupe_set_faces(group)


def load_card_groups(file_path: str):
    root = load_json(file_path)
    data = root.get("data", {})
    if "cards" in data:
        yield from groups_from_set_cards(data["cards"])
    else:
        for name, cards in data.items():
            if isinstance(cards, list):
                yield name, cards


def process_group(cards: list, result: dict) -> None:
    faces = [c for c in cards if isinstance(c, dict) and not c.get("isFunny")]
    if not faces or len(faces) > 2:
        return

    if len(faces) == 1:
        source = faces[0]
        face = build_oracle_face(source, oracle_id(source))
        insert_face(result, face["name"].lower(), make_entry(face))
        return

    if len(faces) == 2 and faces[1]['subtypes'] == ['Omen']:
        # fix Omen -> adventure layout. in mtgjson data they have layout 'reversible_card'
        layouts = {'adventure', }
    else:
        layouts = {f.get("layout", "normal") for f in faces}
    if layouts <= MULTI_LAYOUTS:
        oid = oracle_id(faces[0])
        layout_str = 'adventure' if layouts == {'adventure', } else faces[0].get("layout")
        face_a = build_oracle_face_multi(faces[0], oid)
        face_b = build_oracle_face_multi(faces[1], oid)

        for idx, face in enumerate([face_a, face_b]):
            insert_face(
                result,
                face["name"].lower(),
                make_entry(
                    face,
                    layout=layout_str,
                    face_index=idx,
                ),
            )
        return

    if all(f.get("layout", "normal") not in MULTI_LAYOUTS for f in faces):
        # name collisions
        source = max(faces, key=legality_score)
        face = build_oracle_face(source, oracle_id(source))
        insert_face(result, face["name"].lower(), make_entry(face))


def resolve_set_input(set_name: str) -> str:
    for candidate in (f"{set_name}.json.gz", f"{set_name}.json"):
        if os.path.exists(candidate):
            return candidate
    return f"{set_name}.json.gz"


def ensure_source(file_path: str, source_url: str) -> None:
    if os.path.exists(file_path):
        return
    print(f"Downloading {source_url} -> {file_path}")
    tmp_path = file_path + ".tmp"
    try:
        urllib.request.urlretrieve(source_url, tmp_path)
        os.replace(tmp_path, file_path)
    except Exception:
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
        raise


def process_cards(file_path: str, output_path: str) -> None:
    print(f"Processing cards from: {file_path}")
    result = {}
    for name, cards in load_card_groups(file_path):
        try:
            process_group(cards, result)
        except Exception as exc:
            print(f"Skipping {name}: {exc}")
            return

    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    print(f"Wrote {len(result)} faces to {output_path}")


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        prog="phase-gen",
        description="Generate card-data.json from MTGJSON AtomicCards or a single set.",
    )
    parser.add_argument(
        "-i",
        "--input",
        default=None,
        help="Path to AtomicCards.json.gz or a set JSON (downloaded from MTGJSON if missing)",
    )
    parser.add_argument(
        "-o",
        "--output",
        default=OUTPUT_PATH,
        help="Path to write card-data.json",
    )
    parser.add_argument(
        "--set",
        dest="set_name",
        help="MTGJSON set code (e.g. HOB). Downloads https://mtgjson.com/api/v5/{SET}.json.gz "
        "and reads cards from data.cards",
    )
    args = parser.parse_args(argv)
    if args.set_name:
        set_name = args.set_name.upper()
        source_url = SET_SOURCE_URL.format(set_name=set_name)
        input_path = args.input if args.input is not None else resolve_set_input(set_name)
    else:
        source_url = SOURCE_URL
        input_path = args.input if args.input is not None else INPUT_PATH

    ensure_source(input_path, source_url)
    process_cards(input_path, args.output)


if __name__ == "__main__":
    main()
