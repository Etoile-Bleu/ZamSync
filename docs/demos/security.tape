Output docs/demos/security.gif

Set FontSize 13
Set Width 1100
Set Height 620
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
Type "mkdir -p /tmp/security && cd /tmp/security"
Enter
Sleep 500ms
Type "export PS1='$(tput setaf 5)Etoile-Bleu $(tput setaf 6)\w $(tput setaf 5)❯$(tput sgr0) '"
Enter
Type "info() { echo; tput setaf 6; echo '[*]' $*; tput sgr0; }"
Enter

# Hub acts as CA for the deployment
Type "zamsync keygen ./hub"
Enter
Sleep 500ms

# Rogue node: completely independent CA, never signed by hub
Type "zamsync keygen ./rogue"
Enter
Sleep 500ms

Type "clear"
Enter
Sleep 100ms

Show

# ─────────────────────────────
# 1. Sign a legitimate clinic with hub CA
# ─────────────────────────────

Type "info 'Sign clinic-a certificate with hub CA'"
Enter
Sleep 500ms

Type "zamsync sign ./clinic-a --ca ./hub"
Enter
Sleep 1000ms

# ─────────────────────────────
# 2. Start hub with mTLS
# ─────────────────────────────

Type "zamsync serve ./hub 127.0.0.1:7000 --tls &"
Enter
Sleep 1500ms

# ─────────────────────────────
# 3. Legit clinic-a syncs successfully
# ─────────────────────────────

Type "zamsync submit ./clinic-a admission-P001"
Enter
Sleep 500ms

Type "info 'clinic-a has hub-signed cert -- handshake accepted'"
Enter
Sleep 600ms

Type "zamsync sync ./clinic-a 127.0.0.1:7000 $(cat ./hub/.node_id) --tls"
Enter
Sleep 3000ms

# ─────────────────────────────
# 4. Rogue node rejected at TLS handshake
# ─────────────────────────────

Type "info 'rogue has its own CA -- not trusted by hub'"
Enter
Sleep 600ms

Type "zamsync sync ./rogue 127.0.0.1:7000 $(cat ./hub/.node_id) --tls 2>&1"
Enter
Sleep 5000ms

Type "info 'No data exchanged -- rejected before the handshake completes'"
Enter
Sleep 2000ms
