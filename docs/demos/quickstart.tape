Output docs/demos/quickstart.gif

Set FontSize 13
Set Width 1000
Set Height 560
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
Type "mkdir -p /tmp/quickstart && cd /tmp/quickstart"
Enter
Sleep 500ms
Type "export PS1='$(tput setaf 5)Etoile-Bleu $(tput setaf 6)\w $(tput setaf 5)❯$(tput sgr0) '"
Enter
Type "info() { echo; tput setaf 6; echo '[*]' $*; tput sgr0; }"
Enter

Type "zamsync keygen ./hub"
Enter
Sleep 500ms

Type "zamsync keygen ./clinic"
Enter
Sleep 500ms

Type "zamsync serve ./hub 127.0.0.1:7000 &"
Enter
Sleep 1200ms

Type "clear"
Enter
Sleep 100ms

Show

# ─────────────────────────────
# Write events offline
# ─────────────────────────────

Type "printf '{patient:P-001,ward:3B,type:admission}' > e1.txt"
Enter
Sleep 300ms

Type "printf '{patient:P-002,ward:3B,type:discharge}' > e2.txt"
Enter
Sleep 300ms

Type "printf '{patient:P-003,ward:ICU,type:critical}' > e3.txt"
Enter
Sleep 500ms

Type "info 'Offline event collection'"
Enter
Sleep 500ms

Type "zamsync submit ./clinic $(cat e1.txt)"
Enter
Sleep 400ms

Type "zamsync submit ./clinic $(cat e2.txt)"
Enter
Sleep 400ms

Type "zamsync submit ./clinic $(cat e3.txt)"
Enter
Sleep 800ms

# ─────────────────────────────
# Sync on reconnect
# ─────────────────────────────

Type "info 'Connection restored -- syncing'"
Enter
Sleep 600ms

Type "zamsync sync ./clinic 127.0.0.1:7000 $(cat ./hub/.node_id)"
Enter
Sleep 2500ms

# ─────────────────────────────
# Verify on hub
# ─────────────────────────────

Type "info 'Hub received all events'"
Enter
Sleep 600ms

Type "zamsync info ./hub"
Enter
Sleep 1500ms

Type "zamsync audit ./hub --format json"
Enter
Sleep 2000ms
