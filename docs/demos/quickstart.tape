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

Type "cp /vhs/zamsync-linux-x86_64 /usr/local/bin/zamsync && chmod +x /usr/local/bin/zamsync"
Enter
Sleep 500ms

Type "zamsync keygen ./hub"
Enter
Sleep 500ms

Type "zamsync keygen ./clinic"
Enter
Sleep 500ms

Type "zamsync serve ./hub 127.0.0.1:7000 &"
Enter
Sleep 1200ms

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

Type "# Offline event collection"
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

Type "# Connection restored -- syncing"
Enter
Sleep 600ms

Type "zamsync sync ./clinic 127.0.0.1:7000 $(cat ./hub/.node_id)"
Enter
Sleep 2500ms

# ─────────────────────────────
# Verify on hub
# ─────────────────────────────

Type "# Hub received all events"
Enter
Sleep 600ms

Type "zamsync info ./hub"
Enter
Sleep 1500ms

Type "zamsync audit ./hub --format json"
Enter
Sleep 2000ms
