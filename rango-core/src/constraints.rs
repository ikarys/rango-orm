/// A table-level CHECK constraint — raw SQL expression.
#[derive(Debug, Clone)]
pub struct CheckConstraint {
    pub sql: String,
    pub name: String,
}

impl CheckConstraint {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            name: String::new(),
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

/// A table-level UNIQUE constraint — single or multi-column, optional partial condition.
#[derive(Debug, Clone)]
pub struct UniqueConstraint {
    pub fields: Vec<String>,
    pub condition: Option<String>,
    pub name: String,
}

impl UniqueConstraint {
    pub fn on(fields: &[&str]) -> Self {
        Self {
            fields: fields.iter().map(|s| s.to_string()).collect(),
            condition: None,
            name: String::new(),
        }
    }

    /// Add a WHERE condition (partial unique index).
    pub fn condition(mut self, cond: impl Into<String>) -> Self {
        self.condition = Some(cond.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

/// A table-level constraint — either CHECK or UNIQUE.
#[derive(Debug, Clone)]
pub enum Constraint {
    Check(CheckConstraint),
    Unique(UniqueConstraint),
}

impl From<CheckConstraint> for Constraint {
    fn from(c: CheckConstraint) -> Self {
        Constraint::Check(c)
    }
}

impl From<UniqueConstraint> for Constraint {
    fn from(c: UniqueConstraint) -> Self {
        Constraint::Unique(c)
    }
}

/// Sort direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderDir {
    Asc,
    Desc,
}

/// An ordering clause: column + direction.
#[derive(Debug, Clone)]
pub struct OrderBy {
    pub column: String,
    pub dir: OrderDir,
}

/// Ascending order helper.
pub fn asc(col: impl Into<String>) -> OrderBy {
    OrderBy {
        column: col.into(),
        dir: OrderDir::Asc,
    }
}

/// Descending order helper.
pub fn desc(col: impl Into<String>) -> OrderBy {
    OrderBy {
        column: col.into(),
        dir: OrderDir::Desc,
    }
}

/// An index definition.
#[derive(Debug, Clone)]
pub struct IndexDef {
    pub fields: Vec<String>,
    pub name: String,
    pub unique: bool,
}
