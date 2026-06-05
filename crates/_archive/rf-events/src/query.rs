use serde::{Deserialize, Serialize};

// ── Fields ──────────────────────────────────────────────────

/// Queryable fields on LogRecord. Each maps to an indexed SQLite column
/// or a JSON attribute path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Field {
    // Indexed columns
    Timestamp,
    Severity,
    Source,
    EventType,
    Body,
    FreqMhz,
    Talkgroup,
    SourceUnit,
    Nac,
    Encrypted,
    Band,
    DeviceKey,
    Classification,
    TraceId,
    SpanId,
    OperationId,
    SiteSessionId,
    ReceiverLat,
    ReceiverLon,

    /// JSON attribute path, e.g. "opcode", "rds_ps", "rule_name".
    /// Queries via `json_extract(attributes, '$.key')`.
    Attribute(String),
}

impl Field {
    /// SQL column name for indexed fields, or json_extract expression for attributes.
    pub fn to_sql(&self) -> String {
        match self {
            Field::Timestamp => "timestamp_ns".to_string(),
            Field::Severity => "severity".to_string(),
            Field::Source => "source".to_string(),
            Field::EventType => "event_type".to_string(),
            Field::Body => "body".to_string(),
            Field::FreqMhz => "freq_mhz".to_string(),
            Field::Talkgroup => "talkgroup".to_string(),
            Field::SourceUnit => "source_unit".to_string(),
            Field::Nac => "nac".to_string(),
            Field::Encrypted => "encrypted".to_string(),
            Field::Band => "band".to_string(),
            Field::DeviceKey => "device_key".to_string(),
            Field::Classification => "classification".to_string(),
            Field::TraceId => "trace_id".to_string(),
            Field::SpanId => "span_id".to_string(),
            Field::OperationId => "operation_id".to_string(),
            Field::SiteSessionId => "site_session_id".to_string(),
            Field::ReceiverLat => "receiver_lat".to_string(),
            Field::ReceiverLon => "receiver_lon".to_string(),
            Field::Attribute(key) => format!("json_extract(attributes, '$.{key}')"),
        }
    }

    /// Parse a field name string into a Field enum.
    pub fn parse(s: &str) -> Self {
        match s {
            "timestamp" | "timestamp_ns" => Field::Timestamp,
            "severity" => Field::Severity,
            "source" => Field::Source,
            "event_type" | "type" => Field::EventType,
            "body" => Field::Body,
            "freq_mhz" | "freq" => Field::FreqMhz,
            "talkgroup" | "tg" => Field::Talkgroup,
            "source_unit" | "uid" => Field::SourceUnit,
            "nac" => Field::Nac,
            "encrypted" | "enc" => Field::Encrypted,
            "band" => Field::Band,
            "device_key" | "device" => Field::DeviceKey,
            "classification" | "cls" => Field::Classification,
            "trace_id" => Field::TraceId,
            "span_id" => Field::SpanId,
            "operation_id" => Field::OperationId,
            "site_session_id" => Field::SiteSessionId,
            "receiver_lat" | "lat" => Field::ReceiverLat,
            "receiver_lon" | "lon" => Field::ReceiverLon,
            other => Field::Attribute(other.to_string()),
        }
    }
}

// ── Filter ──────────────────────────────────────────────────

/// A query filter predicate. Filters combine with AND by default;
/// use `Or` for explicit disjunction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Filter {
    /// field = value
    Eq(Field, FilterValue),
    /// field != value
    Ne(Field, FilterValue),
    /// field > value
    Gt(Field, FilterValue),
    /// field >= value
    Gte(Field, FilterValue),
    /// field < value
    Lt(Field, FilterValue),
    /// field <= value
    Lte(Field, FilterValue),
    /// field IN (values...)
    In(Field, Vec<FilterValue>),
    /// field NOT IN (values...)
    NotIn(Field, Vec<FilterValue>),
    /// field LIKE '%value%' (case-insensitive substring match)
    Contains(Field, String),
    /// field NOT LIKE '%value%'
    NotContains(Field, String),
    /// field LIKE pattern (SQL LIKE with % and _ wildcards)
    Like(Field, String),
    /// field matches regex
    Regex(Field, String),
    /// field IS NOT NULL
    Exists(Field),
    /// field IS NULL
    NotExists(Field),
    /// Logical NOT
    Not(Box<Filter>),
    /// All filters must match (AND)
    And(Vec<Filter>),
    /// Any filter must match (OR)
    Or(Vec<Filter>),
}

/// A typed value used in filter comparisons.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

// ── Parameterized SQL ───────────────────────────────────────

/// A parameter value for binding to SQL statements.
/// Maps to rusqlite types on the DB side.
#[derive(Debug, Clone)]
pub enum ParamValue {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
}

/// Collects SQL fragments and their bound parameters.
/// Uses `?N` numbered placeholders for rusqlite.
#[derive(Debug, Default)]
pub struct ParamCollector {
    params: Vec<ParamValue>,
}

impl ParamCollector {
    pub fn new() -> Self {
        Self { params: Vec::new() }
    }

    /// Add a parameter and return its `?N` placeholder string.
    pub fn add(&mut self, value: ParamValue) -> String {
        self.params.push(value);
        format!("?{}", self.params.len())
    }

    /// Add a FilterValue as a parameter.
    pub fn add_filter_value(&mut self, v: &FilterValue) -> String {
        match v {
            FilterValue::String(s) => self.add(ParamValue::Text(s.clone())),
            FilterValue::Int(n) => self.add(ParamValue::Int(*n)),
            FilterValue::Float(f) => self.add(ParamValue::Float(*f)),
            FilterValue::Bool(b) => self.add(ParamValue::Int(if *b { 1 } else { 0 })),
        }
    }

    /// Consume and return the collected parameters.
    pub fn into_params(self) -> Vec<ParamValue> {
        self.params
    }

    /// Current parameter count.
    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }
}

impl FilterValue {
    /// DEPRECATED: Inline SQL literal. Use `ParamCollector::add_filter_value` instead.
    pub fn to_sql_literal(&self) -> String {
        match self {
            FilterValue::String(s) => format!("'{}'", s.replace('\'', "''")),
            FilterValue::Int(n) => n.to_string(),
            FilterValue::Float(f) => f.to_string(),
            FilterValue::Bool(b) => if *b { "1".to_string() } else { "0".to_string() },
        }
    }
}

impl Filter {
    /// Generate a parameterized SQL WHERE clause fragment.
    /// Appends parameter values to the collector.
    pub fn to_param_sql(&self, pc: &mut ParamCollector) -> String {
        match self {
            Filter::Eq(f, v) => {
                let ph = pc.add_filter_value(v);
                format!("{} = {ph}", f.to_sql())
            }
            Filter::Ne(f, v) => {
                let ph = pc.add_filter_value(v);
                format!("{} != {ph}", f.to_sql())
            }
            Filter::Gt(f, v) => {
                let ph = pc.add_filter_value(v);
                format!("{} > {ph}", f.to_sql())
            }
            Filter::Gte(f, v) => {
                let ph = pc.add_filter_value(v);
                format!("{} >= {ph}", f.to_sql())
            }
            Filter::Lt(f, v) => {
                let ph = pc.add_filter_value(v);
                format!("{} < {ph}", f.to_sql())
            }
            Filter::Lte(f, v) => {
                let ph = pc.add_filter_value(v);
                format!("{} <= {ph}", f.to_sql())
            }
            Filter::In(f, vals) => {
                let placeholders: Vec<String> = vals.iter()
                    .map(|v| pc.add_filter_value(v))
                    .collect();
                format!("{} IN ({})", f.to_sql(), placeholders.join(", "))
            }
            Filter::NotIn(f, vals) => {
                let placeholders: Vec<String> = vals.iter()
                    .map(|v| pc.add_filter_value(v))
                    .collect();
                format!("{} NOT IN ({})", f.to_sql(), placeholders.join(", "))
            }
            Filter::Contains(f, s) => {
                let ph = pc.add(ParamValue::Text(format!("%{s}%")));
                format!("{} LIKE {ph}", f.to_sql())
            }
            Filter::NotContains(f, s) => {
                let ph = pc.add(ParamValue::Text(format!("%{s}%")));
                format!("{} NOT LIKE {ph}", f.to_sql())
            }
            Filter::Like(f, pattern) => {
                let ph = pc.add(ParamValue::Text(pattern.clone()));
                format!("{} LIKE {ph}", f.to_sql())
            }
            Filter::Regex(f, pattern) => {
                // SQLite REGEXP requires a custom function registered via rusqlite
                let ph = pc.add(ParamValue::Text(pattern.clone()));
                format!("{} REGEXP {ph}", f.to_sql())
            }
            Filter::Exists(f) => format!("{} IS NOT NULL", f.to_sql()),
            Filter::NotExists(f) => format!("{} IS NULL", f.to_sql()),
            Filter::Not(inner) => format!("NOT ({})", inner.to_param_sql(pc)),
            Filter::And(filters) => {
                let parts: Vec<String> = filters.iter().map(|f| f.to_param_sql(pc)).collect();
                format!("({})", parts.join(" AND "))
            }
            Filter::Or(filters) => {
                let parts: Vec<String> = filters.iter().map(|f| f.to_param_sql(pc)).collect();
                format!("({})", parts.join(" OR "))
            }
        }
    }

    /// Generate a SQL WHERE clause with inline literals (for debug/logging only).
    pub fn to_sql(&self) -> String {
        match self {
            Filter::Eq(f, v) => format!("{} = {}", f.to_sql(), v.to_sql_literal()),
            Filter::Ne(f, v) => format!("{} != {}", f.to_sql(), v.to_sql_literal()),
            Filter::Gt(f, v) => format!("{} > {}", f.to_sql(), v.to_sql_literal()),
            Filter::Gte(f, v) => format!("{} >= {}", f.to_sql(), v.to_sql_literal()),
            Filter::Lt(f, v) => format!("{} < {}", f.to_sql(), v.to_sql_literal()),
            Filter::Lte(f, v) => format!("{} <= {}", f.to_sql(), v.to_sql_literal()),
            Filter::In(f, vals) => {
                let params: Vec<String> = vals.iter().map(|v| v.to_sql_literal()).collect();
                format!("{} IN ({})", f.to_sql(), params.join(", "))
            }
            Filter::NotIn(f, vals) => {
                let params: Vec<String> = vals.iter().map(|v| v.to_sql_literal()).collect();
                format!("{} NOT IN ({})", f.to_sql(), params.join(", "))
            }
            Filter::Contains(f, s) => {
                let escaped = s.replace('\'', "''").replace('%', "\\%");
                format!("LOWER({}) LIKE LOWER('%{}%')", f.to_sql(), escaped)
            }
            Filter::NotContains(f, s) => {
                let escaped = s.replace('\'', "''").replace('%', "\\%");
                format!("LOWER({}) NOT LIKE LOWER('%{}%')", f.to_sql(), escaped)
            }
            Filter::Like(f, pattern) => {
                let escaped = pattern.replace('\'', "''");
                format!("{} LIKE '{}'", f.to_sql(), escaped)
            }
            Filter::Regex(f, pattern) => {
                let escaped = pattern.replace('\'', "''");
                format!("{} REGEXP '{}'", f.to_sql(), escaped)
            }
            Filter::Exists(f) => format!("{} IS NOT NULL", f.to_sql()),
            Filter::NotExists(f) => format!("{} IS NULL", f.to_sql()),
            Filter::Not(inner) => format!("NOT ({})", inner.to_sql()),
            Filter::And(filters) => {
                let parts: Vec<String> = filters.iter().map(|f| f.to_sql()).collect();
                format!("({})", parts.join(" AND "))
            }
            Filter::Or(filters) => {
                let parts: Vec<String> = filters.iter().map(|f| f.to_sql()).collect();
                format!("({})", parts.join(" OR "))
            }
        }
    }
}

// ── Aggregation ─────────────────────────────────────────────

/// Aggregate function for grouped queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggFn {
    Count,
    CountDistinct,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggFn {
    pub fn to_sql(&self, field: &Field) -> String {
        let col = field.to_sql();
        match self {
            AggFn::Count => "COUNT(*)".to_string(),
            AggFn::CountDistinct => format!("COUNT(DISTINCT {col})"),
            AggFn::Sum => format!("SUM({col})"),
            AggFn::Avg => format!("AVG({col})"),
            AggFn::Min => format!("MIN({col})"),
            AggFn::Max => format!("MAX({col})"),
        }
    }
}

/// Aggregation configuration for grouped queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aggregation {
    /// The aggregate function to apply.
    pub function: AggFn,
    /// The field to aggregate (used for Sum/Avg/Min/Max/CountDistinct).
    pub agg_field: Field,
    /// Fields to group by.
    pub group_by: Vec<Field>,
    /// Time bucket size in seconds (for histogram-style grouping).
    /// When set, adds a synthetic `time_bucket` column to GROUP BY.
    pub time_bucket_sec: Option<u64>,
    /// Post-aggregation filter (HAVING clause).
    pub having: Option<Box<Filter>>,
}

// ── Sort ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SortOrder {
    NewestFirst,
    OldestFirst,
}

// ── EventQuery ──────────────────────────────────────────────

/// A complete query against the event log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventQuery {
    /// AND-combined filters.
    pub filters: Vec<Filter>,

    /// Time range (nanosecond timestamps). Both inclusive.
    pub time_range: Option<(u64, u64)>,

    /// Sort order.
    pub order: SortOrder,

    /// Maximum rows to return (default 500).
    pub limit: usize,

    /// Offset for pagination.
    pub offset: usize,

    /// Optional aggregation (turns this into a grouped query).
    pub aggregation: Option<Aggregation>,
}

impl Default for EventQuery {
    fn default() -> Self {
        Self {
            filters: Vec::new(),
            time_range: None,
            order: SortOrder::NewestFirst,
            limit: 500,
            offset: 0,
            aggregation: None,
        }
    }
}

impl EventQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn filter(mut self, f: Filter) -> Self {
        self.filters.push(f);
        self
    }

    pub fn time_range(mut self, start_ns: u64, end_ns: u64) -> Self {
        self.time_range = Some((start_ns, end_ns));
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }

    pub fn offset(mut self, n: usize) -> Self {
        self.offset = n;
        self
    }

    pub fn order(mut self, o: SortOrder) -> Self {
        self.order = o;
        self
    }

    pub fn aggregate(mut self, agg: Aggregation) -> Self {
        self.aggregation = Some(agg);
        self
    }

    /// Generate parameterized SQL and its bind parameters.
    /// Returns `(sql_template, params)` for safe execution.
    pub fn to_param_sql(&self) -> (String, Vec<ParamValue>) {
        let mut pc = ParamCollector::new();
        let sql = if let Some(ref agg) = self.aggregation {
            self.build_agg_sql(agg, &mut pc)
        } else {
            self.build_select_sql(&mut pc)
        };
        (sql, pc.into_params())
    }

    /// Generate parameterized COUNT(*) query for pagination.
    /// Returns `(sql_template, params)`.
    pub fn to_count_sql(&self) -> (String, Vec<ParamValue>) {
        let mut pc = ParamCollector::new();
        let mut sql = String::from("SELECT COUNT(*) FROM event_log");
        let where_clause = self.build_where(&mut pc);
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }
        (sql, pc.into_params())
    }

    fn build_select_sql(&self, pc: &mut ParamCollector) -> String {
        let mut sql = String::from("SELECT * FROM event_log");
        let where_clause = self.build_where(pc);
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }
        let order = match self.order {
            SortOrder::NewestFirst => "DESC",
            SortOrder::OldestFirst => "ASC",
        };
        sql.push_str(&format!(" ORDER BY timestamp_ns {order}"));
        sql.push_str(&format!(" LIMIT {} OFFSET {}", self.limit, self.offset));
        sql
    }

    fn build_agg_sql(&self, agg: &Aggregation, pc: &mut ParamCollector) -> String {
        let agg_expr = agg.function.to_sql(&agg.agg_field);

        let mut group_cols: Vec<String> = agg.group_by.iter().map(|f| f.to_sql()).collect();

        let bucket_expr = agg.time_bucket_sec.map(|sec| {
            let ns = sec * 1_000_000_000;
            format!("(timestamp_ns / {ns}) * {sec}")
        });
        if let Some(ref be) = bucket_expr {
            group_cols.insert(0, format!("{be} AS time_bucket"));
        }

        let select_cols = if group_cols.is_empty() {
            agg_expr.clone()
        } else {
            format!("{}, {agg_expr} AS agg_value", group_cols.join(", "))
        };

        let mut sql = format!("SELECT {select_cols} FROM event_log");
        let where_clause = self.build_where(pc);
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }

        if !group_cols.is_empty() {
            let group_exprs: Vec<String> = {
                let mut exprs = Vec::new();
                if let Some(ref be) = bucket_expr {
                    exprs.push(be.clone());
                }
                for f in &agg.group_by {
                    exprs.push(f.to_sql());
                }
                exprs
            };
            sql.push_str(&format!(" GROUP BY {}", group_exprs.join(", ")));
        }

        if let Some(ref having) = agg.having {
            sql.push_str(&format!(" HAVING {}", having.to_param_sql(pc)));
        }

        if bucket_expr.is_some() {
            sql.push_str(" ORDER BY time_bucket ASC");
        }

        sql.push_str(&format!(" LIMIT {}", self.limit));
        sql
    }

    fn build_where(&self, pc: &mut ParamCollector) -> String {
        let mut parts = Vec::new();

        // Time range using bucket + precise timestamp (SigNoz pattern)
        if let Some((start, end)) = self.time_range {
            let bucket_start = crate::event::bucket_30s(start);
            let bucket_end = crate::event::bucket_30s(end) + 30_000_000_000;
            let ph_bs = pc.add(ParamValue::Int(bucket_start as i64));
            let ph_be = pc.add(ParamValue::Int(bucket_end as i64));
            let ph_ts = pc.add(ParamValue::Int(start as i64));
            let ph_te = pc.add(ParamValue::Int(end as i64));
            parts.push(format!("ts_bucket >= {ph_bs} AND ts_bucket <= {ph_be}"));
            parts.push(format!("timestamp_ns >= {ph_ts} AND timestamp_ns <= {ph_te}"));
        }

        for filter in &self.filters {
            parts.push(filter.to_param_sql(pc));
        }

        parts.join(" AND ")
    }

    /// Generate inline SQL for debug logging. NOT safe for execution with user input.
    pub fn to_sql(&self) -> String {
        if let Some(ref agg) = self.aggregation {
            self.to_agg_sql(agg)
        } else {
            self.to_select_sql()
        }
    }

    fn to_select_sql(&self) -> String {
        let mut sql = String::from("SELECT * FROM event_log");
        let where_clause = self.where_clause();
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }

        let order = match self.order {
            SortOrder::NewestFirst => "DESC",
            SortOrder::OldestFirst => "ASC",
        };
        sql.push_str(&format!(" ORDER BY timestamp_ns {order}"));
        sql.push_str(&format!(" LIMIT {} OFFSET {}", self.limit, self.offset));
        sql
    }

    fn to_agg_sql(&self, agg: &Aggregation) -> String {
        let agg_expr = agg.function.to_sql(&agg.agg_field);

        let mut group_cols: Vec<String> = agg.group_by.iter().map(|f| f.to_sql()).collect();

        let bucket_expr = agg.time_bucket_sec.map(|sec| {
            let ns = sec * 1_000_000_000;
            format!("(timestamp_ns / {ns}) * {sec}")
        });
        if let Some(ref be) = bucket_expr {
            group_cols.insert(0, format!("{be} AS time_bucket"));
        }

        let select_cols = if group_cols.is_empty() {
            agg_expr.clone()
        } else {
            format!("{}, {agg_expr} AS agg_value", group_cols.join(", "))
        };

        let mut sql = format!("SELECT {select_cols} FROM event_log");
        let where_clause = self.where_clause();
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }

        if !group_cols.is_empty() {
            let group_exprs: Vec<String> = {
                let mut exprs = Vec::new();
                if let Some(ref be) = bucket_expr {
                    exprs.push(be.clone());
                }
                for f in &agg.group_by {
                    exprs.push(f.to_sql());
                }
                exprs
            };
            sql.push_str(&format!(" GROUP BY {}", group_exprs.join(", ")));
        }

        if let Some(ref having) = agg.having {
            sql.push_str(&format!(" HAVING {}", having.to_sql()));
        }

        if bucket_expr.is_some() {
            sql.push_str(" ORDER BY time_bucket ASC");
        }

        sql.push_str(&format!(" LIMIT {}", self.limit));
        sql
    }

    fn where_clause(&self) -> String {
        let mut parts = Vec::new();

        if let Some((start, end)) = self.time_range {
            let bucket_start = crate::event::bucket_30s(start);
            let bucket_end = crate::event::bucket_30s(end) + 30_000_000_000;
            parts.push(format!("ts_bucket >= {bucket_start} AND ts_bucket <= {bucket_end}"));
            parts.push(format!("timestamp_ns >= {start} AND timestamp_ns <= {end}"));
        }

        for filter in &self.filters {
            parts.push(filter.to_sql());
        }

        parts.join(" AND ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_select_query() {
        let q = EventQuery::new()
            .filter(Filter::Eq(Field::EventType, FilterValue::String("protocol.p25.grant".to_string())))
            .filter(Filter::Eq(Field::Talkgroup, FilterValue::Int(1001)))
            .limit(100);
        let sql = q.to_sql();
        assert!(sql.contains("event_type = 'protocol.p25.grant'"));
        assert!(sql.contains("talkgroup = 1001"));
        assert!(sql.contains("LIMIT 100"));
    }

    #[test]
    fn parameterized_select_query() {
        let q = EventQuery::new()
            .filter(Filter::Eq(Field::EventType, FilterValue::String("protocol.p25.grant".to_string())))
            .filter(Filter::Eq(Field::Talkgroup, FilterValue::Int(1001)))
            .limit(100);
        let (sql, params) = q.to_param_sql();
        assert!(sql.contains("event_type = ?1"));
        assert!(sql.contains("talkgroup = ?2"));
        assert!(sql.contains("LIMIT 100"));
        assert_eq!(params.len(), 2);
        match &params[0] {
            ParamValue::Text(s) => assert_eq!(s, "protocol.p25.grant"),
            _ => panic!("expected Text"),
        }
        match &params[1] {
            ParamValue::Int(n) => assert_eq!(*n, 1001),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn parameterized_time_range() {
        let q = EventQuery::new()
            .time_range(1000_000_000_000, 2000_000_000_000)
            .filter(Filter::Eq(Field::Band, FilterValue::String("VHF".to_string())))
            .limit(50);
        let (sql, params) = q.to_param_sql();
        assert!(sql.contains("ts_bucket >= ?1"));
        assert!(sql.contains("ts_bucket <= ?2"));
        assert!(sql.contains("timestamp_ns >= ?3"));
        assert!(sql.contains("timestamp_ns <= ?4"));
        assert!(sql.contains("band = ?5"));
        assert_eq!(params.len(), 5);
    }

    #[test]
    fn parameterized_in_filter() {
        let q = EventQuery::new()
            .filter(Filter::In(
                Field::EventType,
                vec![
                    FilterValue::String("p25.voice".to_string()),
                    FilterValue::String("p25.grant".to_string()),
                ],
            ));
        let (sql, params) = q.to_param_sql();
        assert!(sql.contains("IN (?1, ?2)"));
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn parameterized_contains_filter() {
        let q = EventQuery::new()
            .filter(Filter::Contains(Field::Body, "Portland".to_string()));
        let (sql, params) = q.to_param_sql();
        assert!(sql.contains("body LIKE ?1"));
        assert_eq!(params.len(), 1);
        match &params[0] {
            ParamValue::Text(s) => assert_eq!(s, "%Portland%"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn count_query() {
        let q = EventQuery::new()
            .filter(Filter::Eq(Field::Source, FilterValue::Int(1)))
            .time_range(1000, 2000);
        let (sql, params) = q.to_count_sql();
        assert!(sql.starts_with("SELECT COUNT(*) FROM event_log"));
        assert!(sql.contains("ts_bucket"));
        assert!(sql.contains("source = ?5"));
        assert_eq!(params.len(), 5); // 4 time params + 1 filter
    }

    #[test]
    fn aggregation_query() {
        let q = EventQuery::new()
            .filter(Filter::Like(Field::EventType, "protocol.p25.%".to_string()))
            .aggregate(Aggregation {
                function: AggFn::Count,
                agg_field: Field::Timestamp,
                group_by: vec![Field::Talkgroup],
                time_bucket_sec: Some(300),
                having: None,
            })
            .limit(1000);
        let sql = q.to_sql();
        assert!(sql.contains("COUNT(*)"));
        assert!(sql.contains("GROUP BY"));
        assert!(sql.contains("time_bucket"));
    }

    #[test]
    fn filter_in() {
        let f = Filter::In(
            Field::EventType,
            vec![
                FilterValue::String("p25.voice".to_string()),
                FilterValue::String("p25.grant".to_string()),
            ],
        );
        let sql = f.to_sql();
        assert!(sql.contains("IN ('p25.voice', 'p25.grant')"));
    }

    #[test]
    fn field_parse() {
        assert_eq!(Field::parse("tg"), Field::Talkgroup);
        assert_eq!(Field::parse("uid"), Field::SourceUnit);
        assert_eq!(Field::parse("freq"), Field::FreqMhz);
        assert_eq!(Field::parse("opcode"), Field::Attribute("opcode".to_string()));
    }
}
