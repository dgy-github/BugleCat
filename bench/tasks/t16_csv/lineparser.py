from typing import List


class ParseError(ValueError):
    pass


def parse_line(line: str, delimiter: str = ",", quote: str = '"') -> List[str]:
    # NOTE: incomplete naive implementation.
    if not line:
        return []
    fields = []
    for part in line.split(delimiter):
        if part.startswith(quote) and part.endswith(quote):
            part = part[1:-1]
        fields.append(part)
    return fields
