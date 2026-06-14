# Discord Sharing Templates for ZamSync

Here are the templates for sharing ZamSync on Discord, in both English and French.

---

## English Version (International)

Hey everyone!

I'm Mathéo, a 2nd-year computer science student from France. I wanted to share a project I've been building over the last few months: **ZamSync**.

It is a lightweight, offline-first synchronization engine written in Rust. I designed it with a specific real-world case in mind: the electronic Patient Information System (ePIS) of Bhutan. In their remote clinics, the internet is extremely unstable (mostly flaky 2G with high latency). **My ultimate goal is to officially propose this project to the Ministry of Health of Bhutan** to help them sync clinical data reliably without needing heavy database setups on-site.

Under the hood, it's designed to run on low-resource hardware like a Raspberry Pi (under 10MB of RAM):
* An append-only Write-Ahead Log (WAL) encrypted using ChaCha20-Poly1305.
* Hybrid Logical Clocks (HLC) and version vectors to sync missing data deterministically.
* Custom mTLS to secure both client and server nodes.
* A simulation test suite using Toxiproxy to mimic a real Bhutanese 2G network with 600ms latency and random cuts.

Everything is open-source, and I'd love to get your feedback on the architecture. If you like the project, leaving a star ⭐️ on GitHub would mean a lot!
* GitHub: https://github.com/Etoile-Bleu/ZamSync
* Detailed Dev.to article: https://dev.to/etoile_bleu/simulating-2g-to-build-an-offline-first-sync-engine-in-rust-for-rural-clinics-38bd

Let me know what you think!

---

## French Version (Francophone)

Salut tout le monde !

Je m'appelle Mathéo, je suis étudiant en 2ème année à EPITECH Nancy. Je voulais vous partager un projet open-source sur lequel je bosse depuis quelques mois et qui me tient vraiment à cœur : **ZamSync**.

C'est un moteur de synchronisation "offline-first" super léger écrit en Rust. L'idée m'est venue en m'intéressant au système de santé du Bhoutan (le ePIS) : dans leurs cliniques rurales très isolées, la connexion internet est catastrophique (souvent de la 2G instable avec énormément de pertes et de latence). **Mon objectif à long terme est de proposer directement ce projet au Ministère de la Santé du Bhoutan** afin de fiabiliser la synchronisation des dossiers médicaux de leurs patients sans dépendre d'une connexion permanente ou d'infrastructures lourdes sur place.

Côté technique, j'ai voulu faire un truc hyper robuste qui tourne sur du tout petit matériel (comme un Raspberry Pi avec moins de 10 Mo de RAM) :
* Un Write-Ahead Log (WAL) chiffré en ChaCha20-Poly1305 pour stocker les événements localement.
* Des Horloges Logiques Hybrides (HLC) et des vecteurs de version pour ordonner et synchroniser les données sans doublon, même après des jours de coupure.
* Du mTLS pour sécuriser tout le transit.
* Et pour tester tout ça en conditions réelles, j'ai simulé des connexions 2G bhoutanaises pourries en intégrant Toxiproxy dans mes tests d'intégration.

Le projet est entièrement open-source. Si le projet vous plaît, n'hésitez pas à laisser une petite étoile ⭐️ sur GitHub, ça donne énormément de force !
* GitHub : https://github.com/Etoile-Bleu/ZamSync
* J'ai aussi écrit un article détaillé sur l'architecture et la simulation réseau sur dev.to : https://dev.to/etoile_bleu/simulating-2g-to-build-an-offline-first-sync-engine-in-rust-for-rural-clinics-38bd

N'hésitez pas à me faire vos retours ou à poser des questions, ça m'aide énormément à améliorer le moteur !
