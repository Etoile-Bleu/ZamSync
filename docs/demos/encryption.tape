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

Type "if [ -f /vhs/zamsync-linux-x86_64 ]; then B=/vhs/zamsync-linux-x86_64; elif [ -f ./zamsync-linux-x86_64 ]; then B=./zamsync-linux-x86_64; else B=../../zamsync-linux-x86_64; fi"
Enter
Type "if [ -w /usr/local/bin ]; then cp $B /usr/local/bin/zamsync && chmod +x /usr/local/bin/zamsync; else mkdir -p ~/.local/bin && cp $B ~/.local/bin/zamsync && chmod +x ~/.local/bin/zamsync && export PATH=~/.local/bin:$PATH; fi"
Enter
Type "mkdir -p /tmp/encryption && cd /tmp/encryption"
Enter
Sleep 500ms
Type "export PS1='$(tput setaf 5)Etoile-Bleu $(tput setaf 6)\w $(tput setaf 5)❯$(tput sgr0) '"
Enter
Type "info() { echo; tput setaf 6; echo '[*]' $*; tput sgr0; }"
Enter

Type "clear"
Enter
Sleep 100ms

Show

# ─────────────────────────────
# 1. Generate node with encryption key
# ─────────────────────────────

Type "info 'WAL encryption at rest -- ChaCha20-Poly1305, 96-bit random nonce per record'"
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

Type "info 'Raw WAL on disk -- encrypted bytes, unreadable without the key'"
Enter
Sleep 600ms

Type "od -A x -t x1z ./secure/events.wal | head -4"
Enter
Sleep 1000ms

# ─────────────────────────────
# 4. Audit without key -- refused
# ─────────────────────────────

Type "info 'Audit without key -- fails'"
Enter
Sleep 600ms

Type "zamsync audit ./secure 2>&1"
Enter
Sleep 1000ms

# ─────────────────────────────
# 5. Audit with key -- decrypted
# ─────────────────────────────

Type "info 'Audit with key -- decrypted and readable'"
Enter
Sleep 600ms

Type "zamsync audit ./secure --key-file ./secure/tls/data.key"
Enter
Sleep 2000ms
