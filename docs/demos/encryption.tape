Output docs/demos/encryption.gif

Set FontSize 13
Set Width 1000
Set Height 580
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 40ms

Hide

# ─────────────────────────────
# Setup (not recorded)
# ─────────────────────────────

Type "cp /vhs/zamsync-linux-x86_64 /usr/local/bin/zamsync && chmod +x /usr/local/bin/zamsync"
Enter
Sleep 500ms

Show

# ─────────────────────────────
# 1. Generate node with encryption key
# ─────────────────────────────

Type "# WAL encryption at rest -- ChaCha20-Poly1305, 96-bit random nonce per record"
Enter
Sleep 600ms

Type "zamsync keygen ./secure"
Enter
Sleep 1000ms

# ─────────────────────────────
# 2. Submit encrypted events
# ─────────────────────────────

Type "zamsync submit ./secure admission-record-P001 --key-file ./secure/tls/data.key"
Enter
Sleep 500ms

Type "zamsync submit ./secure admission-record-P002 --key-file ./secure/tls/data.key"
Enter
Sleep 800ms

# ─────────────────────────────
# 3. Raw WAL on disk -- unreadable
# ─────────────────────────────

Type "# Raw WAL on disk -- encrypted bytes, unreadable without the key"
Enter
Sleep 600ms

Type "od -A x -t x1z ./secure/events.wal | head -4"
Enter
Sleep 1000ms

# ─────────────────────────────
# 4. Audit without key -- refused
# ─────────────────────────────

Type "# Audit without key -- fails"
Enter
Sleep 600ms

Type "zamsync audit ./secure 2>&1"
Enter
Sleep 1000ms

# ─────────────────────────────
# 5. Audit with key -- decrypted
# ─────────────────────────────

Type "# Audit with key -- decrypted and readable"
Enter
Sleep 600ms

Type "zamsync audit ./secure --key-file ./secure/tls/data.key"
Enter
Sleep 2000ms
