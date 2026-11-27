# LogiMxPlus (Python reference implementation)

The original Python implementation. It is kept as a reference while the Rust version at
the repo root takes over. See the top-level [README](../README.md).

```powershell
cd python
uv sync
uv run ruff check . ; uv run mypy ; uv run pytest
```
