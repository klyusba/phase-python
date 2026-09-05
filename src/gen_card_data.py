# source: 'https://mtgjson.com/api/v5/AtomicCards.json.gz'
import gzip
import json
import os

from phase import build_oracle_face, build_oracle_face_multi


def stream_mtg_cards(gz_file_path: str):
    """Stream MTG cards from AtomicCards.json.gz format.
    
    Yields individual card objects from the nested structure:
    {"meta": {...}, "data": {"cardName": [card1, card2, ...]}}
    """
    with gzip.open(gz_file_path, 'r') as f:
        root = json.load(f)
    data = root.get('data', {})

    for card_name, cards in data.items():
        if not isinstance(cards, list):
            print(f"Skipping card: {card_name}")
            continue
        if len(cards) == 1:
            yield cards[0]
        elif len(cards) == 2:
            # there is a known issue, for different cards with same name: remove playtest cards
            yield cards


def process_cards(gz_file_path: str):
    """Process cards from a gzipped JSON file."""
    if not os.path.exists(gz_file_path):
        print(f"File not found: {gz_file_path}")
        return

    print(f"Processing cards from: {gz_file_path}")
    result = {}
    for card in stream_mtg_cards(gz_file_path):
        if isinstance(card, dict):
            oracle_id = card.get('identifiers', {}).get('scryfallOracleId', None)
            face = build_oracle_face(card, oracle_id)
            key = face['name'].lower()
            result.setdefault(key, face)
        elif isinstance(card, list):
            # multifaced card
            oracle_id = card[0].get('identifiers', {}).get('scryfallOracleId', None)
            face_a = build_oracle_face_multi(card[0], oracle_id)
            face_b = build_oracle_face_multi(card[1], oracle_id)
            for face in [face_a, face_b]:
                key = face['name'].lower()
                result.setdefault(key, face)


if __name__ == '__main__':
    script_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.dirname(script_dir)
    gz_file = os.path.join(project_root, 'AtomicCards.json.gz')
    process_cards(gz_file)
