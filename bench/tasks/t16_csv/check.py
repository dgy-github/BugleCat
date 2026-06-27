import importlib
m = importlib.import_module("lineparser")
parse_line = m.parse_line
ParseError = m.ParseError

def eq(line, expected, **kw):
    got = parse_line(line, **kw)
    assert got == expected, f"parse_line({line!r}, {kw}) -> {got!r}, expected {expected!r}"

def err(line, **kw):
    try:
        parse_line(line, **kw)
    except ParseError:
        return
    except Exception as e:
        raise AssertionError(f"parse_line({line!r}) raised {type(e).__name__}, expected ParseError")
    raise AssertionError(f"parse_line({line!r}) did not raise, expected ParseError")

# Basic
eq("a,b,c", ["a", "b", "c"])
eq("", [])
eq("a", ["a"])
eq(",", ["", ""])
eq(",,", ["", "", ""])
eq("a,", ["a", ""])
eq(",a", ["", "a"])

# Quoted fields
eq('"a","b"', ["a", "b"])
eq('"a,b",c', ["a,b", "c"])
eq('"hello world"', ["hello world"])
eq('""', [""])
eq('"",""', ["", ""])

# Escaped quotes (doubled)
eq('"a""b"', ['a"b'])
eq('""""', ['"'])
eq('"say ""hi"""', ['say "hi"'])
eq('"a""""b"', ['a""b'])

# Mixed quoted/unquoted across fields
eq('a,"b,c",d', ["a", "b,c", "d"])
eq('"a",b,"c"', ["a", "b", "c"])

# Whitespace preserved
eq(' a , b ', [" a ", " b "])
eq('" a "," b "', [" a ", " b "])

# Custom delimiter / quote
eq("a;b;c", ["a", "b", "c"], delimiter=";")
eq("'a;b';c", ["a;b", "c"], delimiter=";", quote="'")
eq("'it''s';ok", ["it's", "ok"], delimiter=";", quote="'")

# Delimiter right after a closed quote
eq('"a",', ["a", ""])
eq('"a","b",', ["a", "b", ""])

# Error: unterminated quote
err('"abc')
err('"a,b')
err('"a""')
# Error: text after closing quote
err('"a"b')
err('"a" b')
# Error: quote starting mid-field
err('ab"c"')
err('a"b')
# Error: bad config
err("a,b", delimiter="")
err("a,b", quote="||")
err("a,b", delimiter=",", quote=",")

print("ok")
