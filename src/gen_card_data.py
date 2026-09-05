# simplified version of crates/engine/src/bin/oracle_gen.rs
# expects data from https://mtgjson.com/api/v5/AtomicCards.json.gz
import gzip
import json
import os

from phase import build_oracle_face, build_oracle_face_multi


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

FORMAT_KEYS = {
    "standard",
    "commander",
    "modern",
    "premodern",
    "pioneer",
    "legacy",
    "vintage",
    "pauper",
    "historic",
    "brawl",
    "standardbrawl",
    "timeless",
    "paupercommander",
    "duel",
    "oathbreaker",
}

STATUS_EXPORT = {
    "legal": "legal",
    "notlegal": "not_legal",
    "banned": "banned",
    "restricted": "restricted",
}


def alnum_lower(value: str) -> str:
    return "".join(c.lower() for c in value if c.isalnum())


def export_legalities(legalities: dict | None) -> dict:
    result = {}
    if not legalities:
        return result
    for key, value in legalities.items():
        fmt = alnum_lower(key)
        if fmt not in FORMAT_KEYS:
            continue
        status = STATUS_EXPORT.get(alnum_lower(str(value)))
        if status:
            result[fmt] = status
    return result


def legality_score(card: dict) -> int:
    return sum(1 for status in (card.get("legalities") or {}).values() if status.lower() == "legal")


def make_entry(face: dict, source: dict, layout=None, face_index=None, rulings=None) -> dict:
    entry = dict(face)
    entry["legalities"] = export_legalities(source.get("legalities"))
    if layout:
        entry["layout"] = layout
    if face_index is not None:
        entry["face_index"] = face_index
    printings = source.get("printings") or []
    if printings:
        entry["printings"] = printings
    if rulings:
        entry["rulings"] = [
            {"date": r["date"], "text": r["text"]}
            for r in rulings
            if isinstance(r, dict) and "date" in r and "text" in r
        ]
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
        insert_face(result, face["name"].lower(), make_entry(face, source, rulings=source.get("rulings")))
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
                    faces[0],
                    layout=layout_str,
                    face_index=idx,
                    rulings=faces[0].get("rulings") if idx == 0 else None,
                ),
            )
        return

    if all(f.get("layout", "normal") not in MULTI_LAYOUTS for f in faces):
        # name collisions
        source = max(faces, key=legality_score)
        face = build_oracle_face(source, oracle_id(source))
        insert_face(result, face["name"].lower(), make_entry(face, source, rulings=source.get("rulings")))


def process_cards(gz_file_path: str, output_path: str) -> None:
    if not os.path.exists(gz_file_path):
        print(f"File not found: {gz_file_path}")
        return

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


if __name__ == "__main__":
    script_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.dirname(script_dir)
    gz_file = os.path.join(project_root, "AtomicCards.json.gz")
    out_file = os.path.join(project_root, "card-data.json")
    process_cards(gz_file, out_file)
