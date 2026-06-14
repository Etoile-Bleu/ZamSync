/// Controls which events a serving node sends to connecting peers.
///
/// `All` (default): every peer receives all events it is missing -- suitable
/// for hub-to-hub replication or fully trusted deployments.
///
/// `OwnOnly`: a peer P only receives events whose `origin_node == P`. This is
/// the "hub in clinic mode": clinic A can upload its records to the hub and
/// retrieve them back, but cannot read records from clinic B.
#[derive(Debug, Clone, Default)]
pub enum AccessPolicy {
    #[default]
    All,
    OwnOnly,
}

impl std::str::FromStr for AccessPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "all" => Ok(Self::All),
            "own" => Ok(Self::OwnOnly),
            other => Err(format!("unknown policy '{other}': use 'all' or 'own'")),
        }
    }
}
