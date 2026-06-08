# Reproduce A servant-rs Issue

Use this command to turn a bug report or suspected behavior gap into a minimal reproduction.

User report: `$ARGUMENTS`

## Procedure

1. Restate the observed behavior and expected behavior in concrete terms.
2. Locate the Rust code path and the matching Haskell reference path under `research/servant/`.
3. Create or identify the smallest failing test.
4. Run the failing test and capture the important output.
5. Fix only the behavior needed for the reproduction.
6. Re-run the focused test and any nearby tests.

## Output

Report the failing case, the fix, and the verification commands. Include remaining uncertainty if the behavior differs intentionally from Haskell Servant.
