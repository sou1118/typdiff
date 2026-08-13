from pathlib import Path

import pytest

import typdiff

FIXTURES = Path(__file__).parents[3] / "tests" / "fixtures"


def test_diff():
    old = (FIXTURES / "old.typ").read_bytes()
    new = (FIXTURES / "new.typ").read_bytes()

    output = typdiff.diff(old, new)

    assert isinstance(output, bytes)
    assert b"#diff-deleted[Introduction]" in output
    assert b"#diff-added[Background]" in output
    assert b"#diff-deleted[Second]#diff-added[Third] item" in output


@pytest.mark.parametrize("as_path", [str, Path])
def test_diff_files(as_path):
    output = typdiff.diff_files(as_path(FIXTURES / "old.typ"), as_path(FIXTURES / "new.typ"))

    assert output == typdiff.diff(
        (FIXTURES / "old.typ").read_bytes(), (FIXTURES / "new.typ").read_bytes()
    )


def test_diff_files_missing():
    with pytest.raises(OSError):
        typdiff.diff_files(str(FIXTURES / "does-not-exist.typ"), str(FIXTURES / "new.typ"))


def test_diff_invalid_utf8():
    with pytest.raises(ValueError):
        typdiff.diff(b"\xff\xfe", b"new")
