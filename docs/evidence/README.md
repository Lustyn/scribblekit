# Evidence ledgers

Every field name, enum/flag name and semantic claim in the codecs should be backed by the game
itself. One ledger per area (`object.md`, `map.md`, `ui-effects.md`, `words-art.md`,
`placeholders.md` for `gift.vec` placeholders and ropes), each a table:

| Format / type | Field or claim | Evidence | Kind |
|---|---|---|---|

* **Evidence** cites the engine: `FUN_xxxxxxxx` (+ what it does with the value, e.g. "compares
  `+0x2c` with 3 then plays the swim anim"), a data address (`DAT_...`, string table contents),
  or a game text string. Quote the decompiled line when it is short.
* **Kind**: `code` (read and used by the engine as described), `unused` (the loader reads and
  discards it / never reads it — cite the loader), `text` (the game's own string names it), or
  `data-only` (no code path found; say what was searched and why the name is still the best
  reading).

Decompilation: `re/decompiled.c` (grep `// ==== FUN_xxxxxxxx`). The Ghidra project in
`re/project` can be re-opened headless for xrefs:
`tools/ghidra_*/support/analyzeHeadless re/project Scribble -process Scribble.exe -noanalysis -scriptPath <dir> -postScript <Script.java> [args]`.
`rizin -A game/Scribble.exe` works for disassembly and `axt` xrefs.
