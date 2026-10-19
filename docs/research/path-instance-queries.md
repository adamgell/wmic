# Keyed PATH GET queries

A direct command such as `path Win32_Process.Handle='4294967295' get Caption` previously sent `SELECT Caption FROM Win32_Process.Handle='4294967295'` to WMI. Microsoft WMIC sends `SELECT Caption FROM Win32_Process WHERE Handle='4294967295'`. The former query fails instead of looking up the selected instance.

The query builder now moves the single key binding into a WHERE condition for direct PATH GET requests, including implicit GET and GET with no property list. It preserves the literal's original UTF-16 units and quote spelling. Quoted dots and the word `WHERE` inside a value stay inside the value.

## Measured scope

The [16-case manifest](../../tests/fixtures/path-instance-cases.json) contains eight commands with TRACE on and the same eight commands without TRACE. The [original captures](../../tests/fixtures/path-instance-reference/) retain full stdout, stderr and DWORD exit statuses, with exact invocation and stream hashes. Seven shapes succeed: double-quoted, single-quoted and unquoted numeric keys; explicit and implicit all-property GET; and quoted values containing a dot or `WHERE`. An absent process ID and absent environment names avoid dependence on changing instance data.

The eighth shape is a negative control: `path Win32_OperatingSystem=@ get Caption`. The measured Microsoft executable keeps `Win32_OperatingSystem=@` in FROM and returns `0x80041017` (invalid query). The fix preserves that behavior.

Additional original-only probes showed that comma-bearing paths are rejected, including a comma inside a quoted value. Escaped quotes are consumed differently by the native parser. Those forms retain their previous query assembly and are not counted as passing compatibility cases here. Aliases, CLASS requests, additional WHERE selectors, and method/write requests also retain their previous query assembly. This change does not claim compatibility for those forms. Namespace-qualified paths remain subject to existing class-resolution behavior; the helper does not implement a complete object-path grammar.

## Regression tests

`cargo test --locked --test path_instance_queries` runs six portable tests. They compare generated queries to native TRACE captures, check raw UTF-16 preservation, protect the excluded request forms, and drive each plain command through Session parsing and dispatch. The controlled backend asserts the exact query and one invocation before returning the measured empty result or singleton error; the tests then compare complete output and exit status.

On Windows, an additional test launches the actual CLI for all eight plain commands and compares both streams and full status to the original captures. Pipe readers drain concurrently; a 15-second timeout terminates and reaps the child. The invalid FROM query from the original bug cannot pass these empty-result cases because its error status and output differ. These cases do not establish returned-instance values or every property/rendering path.

Select `--suite path-instance` in `tools/reference/run_corpus.py` for a full differential capture. TRACE cases for implicit/all-property GET can also exercise the separate PATH preflight ordering work in PR #14; the query-builder assertions here do not establish that full diagnostic sequence.

## Reference and validation limits

The captures were made on 2026-10-19 with the pinned ARM64 Microsoft executable, in the existing selected Windows reference environment. The recorded profile is SYSTEM, en-US, ANSI 1252, OEM/console 437, CreateProcessW and native pipes. The executable SHA256 is `2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c`; the English MUI SHA256 is `5e3bc78af6eb1aea40c6899c452ca8701d28756bf1eb5f056207fc963cfea17b`. Original executables and resource files are not distributed.

Local Rust/Python tests and ARM64, x64 and x86 Windows compilation are separate from running those three candidate binaries against Microsoft WMIC. That native comparison remains pending while Microsoft reviews the earlier Defender detection associated with PR #14. This fix was developed independently from main and does not change PR #14 or the pending sample. No claim of complete WMIC or object-path compatibility follows from these tests.
