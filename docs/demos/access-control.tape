Output docs/demos/access-control.gif

Set FontSize 13
Set Width 1100
Set Height 680
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 38ms

Hide

# ─────────────────────────────
# Setup (not recorded)
# ─────────────────────────────

Type "if [ -f /vhs/zamsync-linux-x86_64 ]; then B=/vhs/zamsync-linux-x86_64; elif [ -f ./zamsync-linux-x86_64 ]; then B=./zamsync-linux-x86_64; else B=../../zamsync-linux-x86_64; fi"
Enter
Type "if [ -w /usr/local/bin ]; then cp $B /usr/local/bin/zamsync && chmod +x /usr/local/bin/zamsync; else mkdir -p ~/.local/bin && cp $B ~/.local/bin/zamsync && chmod +x ~/.local/bin/zamsync && export PATH=~/.local/bin:$PATH; fi"
Enter
Type "mkdir -p /tmp/access-control && cd /tmp/access-control"
Enter
Sleep 500ms
Type "export PS1='$(tput setaf 5)Etoile-Bleu $(tput setaf 6)\w $(tput setaf 5)❯$(tput sgr0) '"
Enter
Type "info() { echo; tput setaf 6; echo '[*]' $*; tput sgr0; }"
Enter

Type "zamsync keygen ./hub"
Enter
Sleep 400ms

Type "zamsync sign ./clinic-a --ca ./hub"
Enter
Sleep 400ms

Type "zamsync sign ./clinic-b --ca ./hub"
Enter
Sleep 400ms

# Hub with --policy own: each node only receives back its own events
Type "zamsync serve ./hub 127.0.0.1:7000 --policy own &"
Enter
Sleep 1200ms

Type "clear"
Enter
Sleep 100ms

Show

# ─────────────────────────────
# clinic-a submits 3 events offline
# ─────────────────────────────

Type "info 'clinic-a: 3 patient records collected offline'"
Enter
Sleep 500ms

Type "zamsync submit ./clinic-a ward-3B-admission-P001"
Enter
Sleep 300ms

Type "zamsync submit ./clinic-a ward-3B-admission-P002"
Enter
Sleep 300ms

Type "zamsync submit ./clinic-a ward-ICU-critical-P003"
Enter
Sleep 600ms

Type "zamsync sync ./clinic-a 127.0.0.1:7000 $(cat ./hub/.node_id)"
Enter
Sleep 2500ms

# ─────────────────────────────
# clinic-b submits 2 events offline
# ─────────────────────────────

Type "info 'clinic-b: 2 patient records collected offline'"
Enter
Sleep 500ms

Type "zamsync submit ./clinic-b ward-5A-admission-P010"
Enter
Sleep 300ms

Type "zamsync submit ./clinic-b ward-5A-discharge-P011"
Enter
Sleep 600ms

Type "zamsync sync ./clinic-b 127.0.0.1:7000 $(cat ./hub/.node_id)"
Enter
Sleep 2500ms

# ─────────────────────────────
# Verify isolation
# ─────────────────────────────

Type "info 'Hub sees ALL 5 events from both clinics'"
Enter
Sleep 600ms

Type "zamsync audit ./hub"
Enter
Sleep 2000ms

Type "info 'clinic-a sees only its own 3 events -- not clinic-b'"
Enter
Sleep 600ms

Type "zamsync audit ./clinic-a"
Enter
Sleep 1500ms

Type "info 'clinic-b sees only its own 2 events -- not clinic-a'"
Enter
Sleep 600ms

Type "zamsync audit ./clinic-b"
Enter
Sleep 2000ms
