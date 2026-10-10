import pytest

from evals.verification.collection_manifest import interfaces


def test_public_interface_contains_no_reference_body_or_annotation():
    result = interfaces("def f(a: int, b=2, *, c=None):\n return 'SECRET_REFERENCE_ALGORITHM'")
    assert result == ["def f(a, b=2, *, c=None):"]
    assert "SECRET" not in str(result)


def test_executable_defaults_require_review():
    with pytest.raises(ValueError, match="nonliteral default"):
        interfaces("def f(x=helper()): return x")
