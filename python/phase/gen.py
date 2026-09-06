# simplified version of crates/engine/src/bin/oracle_gen.rs
import argparse
import gzip
import json
import os
import urllib.request

from phase import build_oracle_face, build_oracle_face_multi

INPUT_PATH = "AtomicCards.json.gz"
OUTPUT_PATH = "card-data.json"
SOURCE_URL = "https://mtgjson.com/api/v5/AtomicCards.json.gz"

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


def load_atomic_groups(gz_file_path: str):
    with gzip.open(gz_file_path, "r") as f:
        root = json.load(f)
    data = root.get("data", {})
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


def ensure_atomic_cards(gz_file_path: str) -> None:
    if os.path.exists(gz_file_path):
        return
    print(f"Downloading {SOURCE_URL} -> {gz_file_path}")
    tmp_path = gz_file_path + ".tmp"
    try:
        urllib.request.urlretrieve(SOURCE_URL, tmp_path)
        os.replace(tmp_path, gz_file_path)
    except Exception:
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
        raise


def process_cards(gz_file_path: str, output_path: str) -> None:
    ensure_atomic_cards(gz_file_path)

    print(f"Processing cards from: {gz_file_path}")
    result = {}
    for name, cards in load_atomic_groups(gz_file_path):
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
        description="Generate card-data.json from MTGJSON AtomicCards.",
    )
    parser.add_argument(
        "-i",
        "--input",
        default=INPUT_PATH,
        help="Path to AtomicCards.json.gz (downloaded from MTGJSON if missing)",
    )
    parser.add_argument(
        "-o",
        "--output",
        default=OUTPUT_PATH,
        help="Path to write card-data.json",
    )
    args = parser.parse_args(argv)
    process_cards(args.input, args.output)


if __name__ == "__main__":
    main()
