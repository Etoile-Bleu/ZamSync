# ZamSync — Resilient Offline-First Synchronization Engine

## 🌍 Vision du Système
ZamSync est un moteur de synchronisation conçu pour les environnements de santé en zones rurales isolées (Bhoutan). Il est conçu pour être **offline-first**, **event-driven**, et **append-only**, avec une résilience extrême aux coupures réseau et une optimisation pour l'ultra-basse bande passante (2G/LoRa/Satellite).

## 🧩 Problématique Technique
Les protocoles standards (REST, JSON, HTTP) échouent car :
- Trop d'overhead réseau.
- Pas de reprise native au niveau de l'octet (byte-level resume).
- Dépendance à des connexions stables.

## ⚙️ Architecture Système (Deep Engineering)

### 1. Stockage Local & WAL (Write-Ahead Log)
- **Modèle Append-Only** : Toutes les modifications sont d'abord inscrites dans un journal binaire immuable avant d'être appliquées à l'état local.
- **Intégrité** : Chaque enregistrement dispose d'un Checksum (CRC32/XXH3) pour détecter les corruptions dues à des coupures d'énergie.

### 2. Protocole de Synchronisation
- **Vector Clocks** : Utilisation d'horloges vectorielles pour suivre l'état de synchronisation entre les cliniques et le serveur central sans perte de contexte.
- **Delta Sync** : Seuls les octets modifiés ou les nouveaux événements du log sont transmis.

### 3. Transport Réseau Résilient
- **Chunking Stratégique** : Découpage des données en petits fragments (ex: 4KB) avec signature individuelle.
- **Resume-at-Offset** : Capacité de reprendre un transfert exactement là où il s'est arrêté après une déconnexion de plusieurs jours.

### 4. Format Binaire Compact
- **Sérialisation Custom** : Utilisation de formats comme `rkyv` ou `FlatBuffers` pour du zero-copy.
- **Bitpacking & Varints** : Compression des IDs et timestamps pour réduire chaque octet inutile.

---

## 📈 Roadmap de Développement

### Phase 1 : Fondations & Persistance (Priorité Haute)
- **Tâche 1.1** : Initialisation du workspace Rust (crates: core, storage, network).
- **Tâche 1.2** : Implémentation du WAL binaire avec validation d'intégrité.
- **Tâche 1.3** : Moteur de stockage minimal pour les dossiers médicaux.

### Phase 2 : Logique de Synchronisation
- **Tâche 2.1** : Système de versionnement par Vector Clocks.
- **Tâche 2.2** : Algorithme de détection de deltas entre deux nœuds.
- **Tâche 2.3** : Résolution de conflits simple (Last Writer Wins).

### Phase 3 : Transport & Réseau
- **Tâche 3.1** : Design du protocole binaire (Headers minimaux).
- **Tâche 3.2** : Implémentation du chunking et mécanisme de "Resume".
- **Tâche 3.3** : Gestion du backoff adaptatif pour les réseaux instables.

### Phase 4 : Optimisation & Sécurité
- **Tâche 4.1** : Compression Zstd avec dictionnaires pré-entraînés.
- **Tâche 4.2** : Chiffrement de bout en bout (ChaCha20-Poly1305).
- **Tâche 4.3** : Validation de l'intégrité globale (Merkle Trees).

---

## 🧭 Critères de Succès
1. Zéro perte de données en cas de coupure brutale.
2. Reprise automatique d'un transfert après interruption.
3. Consommation mémoire < 100MB pour déploiement sur matériel modeste.
