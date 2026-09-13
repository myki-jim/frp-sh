use super::*;
impl Store {
    /// Only an active, authenticated member can obtain its stable virtual address.
    pub fn virtual_address(
        &self,
        space: &str,
        device: &crate::device::VerifiedDevice,
        now: u64,
    ) -> anyhow::Result<std::net::Ipv4Addr> {
        self.membership(space, device, now)?;
        let address: u8 = self.db.query_row(
            "SELECT address FROM members WHERE space=? AND device=?",
            params![space, device.public_key()],
            |r| r.get(0),
        )?;
        Ok(std::net::Ipv4Addr::new(10, 66, 0, address))
    }
    pub fn list(
        &self,
        device: &crate::device::VerifiedDevice,
        now: u64,
    ) -> anyhow::Result<Vec<Space>> {
        ensure!(now <= i64::MAX as u64, "invalid clock");
        let mut query = self.db.prepare("SELECT s.id,s.name,s.owner,s.expires_at FROM spaces s JOIN members m ON m.space=s.id WHERE m.device=? AND (s.expires_at IS NULL OR s.expires_at>?) ORDER BY s.id LIMIT 1024")?;
        let rows = query.query_map(params![device.public_key(), now as i64], |r| {
            Ok(Space {
                id: r.get(0)?,
                name: r.get(1)?,
                owner: r.get(2)?,
                expires_at: r.get::<_, Option<i64>>(3)?.map(|v| v as u64),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    pub fn membership(
        &self,
        space: &str,
        device: &crate::device::VerifiedDevice,
        now: u64,
    ) -> anyhow::Result<Space> {
        self.list(device, now)?
            .into_iter()
            .find(|s| s.id == space)
            .context("space membership required")
    }
    pub fn members(
        &self,
        space: &str,
        device: &crate::device::VerifiedDevice,
        now: u64,
    ) -> anyhow::Result<Vec<String>> {
        self.membership(space, device, now)?;
        let mut query = self
            .db
            .prepare("SELECT device FROM members WHERE space=? ORDER BY device LIMIT 33")?;
        let rows = query
            .query_map([space], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn delete(
        &mut self,
        space: &str,
        owner: &crate::device::VerifiedDevice,
        now: u64,
    ) -> anyhow::Result<()> {
        let value = self.membership(space, owner, now)?;
        ensure!(value.owner == owner.public_key(), "space owner required");
        self.db.execute(
            "DELETE FROM spaces WHERE id=? AND owner=?",
            params![space, owner.public_key()],
        )?;
        Ok(())
    }
    pub fn leave(
        &mut self,
        space: &str,
        device: &crate::device::VerifiedDevice,
        now: u64,
    ) -> anyhow::Result<()> {
        let value = self.membership(space, device, now)?;
        ensure!(
            value.owner != device.public_key(),
            "owner must delete the space instead of leaving"
        );
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM members WHERE space=? AND device=?",
            params![space, device.public_key()],
        )?;
        tx.execute("DELETE FROM redemptions WHERE device=? AND hash IN (SELECT hash FROM invites WHERE space=?)",params![device.public_key(),space])?;
        tx.commit()?;
        Ok(())
    }
    pub fn revoke_invites(
        &mut self,
        space: &str,
        owner: &crate::device::VerifiedDevice,
        now: u64,
    ) -> anyhow::Result<()> {
        let value = self.membership(space, owner, now)?;
        ensure!(value.owner == owner.public_key(), "space owner required");
        self.db
            .execute("UPDATE invites SET revoked=1 WHERE space=?", [space])?;
        Ok(())
    }
}
