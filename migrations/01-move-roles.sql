ALTER TABLE servers ADD COLUMN allowed_role BIGINT;
ALTER TABLE servers DROP COLUMN name;
