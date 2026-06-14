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

Type "cp /vhs/zamsync-linux-x86_64 /usr/local/bin/zamsync && chmod +x /usr/local/bin/zamsync"
Enter
Sleep 500ms

# Hub acts as CA for the deployment
Type "zamsync keygen ./hub"
Enter
Sleep 500ms

# Rogue node: completely independent CA, never signed by hub
Type "zamsync keygen ./rogue"
Enter
Sleep 500ms

Show

# ─────────────────────────────
# 1. Sign a legitimate clinic with hub CA
# ─────────────────────────────

Type "# Sign clinic-a certificate with hub CA"
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

Type "# clinic-a has hub-signed cert -- handshake accepted"
Enter
Sleep 600ms

Type "zamsync sync ./clinic-a 127.0.0.1:7000 $(cat ./hub/.node_id) --tls"
Enter
Sleep 3000ms

# ─────────────────────────────
# 4. Rogue node rejected at TLS handshake
# ─────────────────────────────

Type "# rogue has its own CA -- not trusted by hub"
Enter
Sleep 600ms

Type "zamsync sync ./rogue 127.0.0.1:7000 $(cat ./hub/.node_id) --tls 2>&1"
Enter
Sleep 5000ms

Type "# No data exchanged -- rejected before the handshake completes"
Enter
Sleep 2000ms
