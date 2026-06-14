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

Type "cp /vhs/zamsync-linux-x86_64 /usr/local/bin/zamsync && chmod +x /usr/local/bin/zamsync"
Enter
Sleep 500ms

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

Show

# ─────────────────────────────
# clinic-a submits 3 events offline
# ─────────────────────────────

Type "# clinic-a: 3 patient records collected offline"
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

Type "# clinic-b: 2 patient records collected offline"
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

Type "# Hub sees ALL 5 events from both clinics"
Enter
Sleep 600ms

Type "zamsync audit ./hub"
Enter
Sleep 2000ms

Type "# clinic-a sees only its own 3 events -- not clinic-b's"
Enter
Sleep 600ms

Type "zamsync audit ./clinic-a"
Enter
Sleep 1500ms

Type "# clinic-b sees only its own 2 events -- not clinic-a's"
Enter
Sleep 600ms

Type "zamsync audit ./clinic-b"
Enter
Sleep 2000ms
