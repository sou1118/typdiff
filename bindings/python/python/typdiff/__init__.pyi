import os

def diff(old: bytes, new: bytes) -> bytes:
    """Diff two Typst documents given as source bytes, returning diff markup as bytes."""

def diff_files(old_path: str | os.PathLike[str], new_path: str | os.PathLike[str]) -> bytes:
    """Diff two Typst documents given as file paths, returning diff markup as bytes."""
