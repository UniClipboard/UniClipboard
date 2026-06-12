use anyhow::Result;

pub trait InsertMapper<D, R>: Sync + Send {
    fn to_row(&self, domain: &D) -> Result<R>;
}

pub trait RowMapper<R, D>: Sync + Send {
    fn to_domain(&self, row: &R) -> Result<D>;
}
