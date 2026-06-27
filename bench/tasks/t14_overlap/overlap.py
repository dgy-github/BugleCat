def find_overlapping(text: str, pattern: str) -> list:
    """Bug: only finds non-overlapping occurrences."""
    indices = []
    start = 0
    while True:
        idx = text.find(pattern, start)
        if idx == -1:
            break
        indices.append(idx)
        start = idx + len(pattern)  # skip past the match – breaks overlap
    return indices
