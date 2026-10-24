string_enum! {
    /// What is known about use; never a statement about expendability.
    pub enum Observation {
        Unknown => "unknown",
        Observed => "observed",
        Review => "review",
        Protected => "protected",
    }
}

impl Observation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unbekannt",
            Self::Observed => "Belegt",
            Self::Review => "Prüfen",
            Self::Protected => "Geschützt",
        }
    }
}

string_enum! {
    /// Data provenance. Demo rows never mix with local rows.
    pub enum Origin {
        Demo => "demo",
        Registered => "registered",
    }
}

string_enum! {
    /// How a resource entered the inventory. Adoption is no cleanup approval.
    pub enum Acquisition {
        Registered => "registered",
        Adopted => "adopted",
        Managed => "managed",
        Discovered => "discovered",
    }
}

impl Acquisition {
    pub fn label(self) -> &'static str {
        match self {
            Self::Registered => "Registriert",
            Self::Adopted => "Übernommen",
            Self::Managed => "Von mogumogu angelegt",
            Self::Discovered => "Passiv entdeckt",
        }
    }
}

string_enum! {
    pub enum ResourceKind {
        Scratchpad => "scratchpad",
        Source => "source",
        Environment => "environment",
        Temporary => "temporary",
        Results => "results",
        Cache => "cache",
        Output => "output",
    }
}

impl ResourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Scratchpad => "Scratchpad",
            Self::Source => "Quellen",
            Self::Environment => "Umgebung",
            Self::Temporary => "Temporär",
            Self::Results => "Ergebnisse",
            Self::Cache => "Cache",
            Self::Output => "Ausgabe",
        }
    }
}

string_enum! {
    /// Explicit owners are local decisions; inferred owners carry evidence.
    pub enum OwnerKind {
        Explicit => "explicit",
        Inferred => "inferred",
    }
}
