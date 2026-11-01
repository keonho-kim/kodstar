-- Purpose: 분석 transaction의 파일·정체성 변화만 기존 집계에 반영.
-- Pattern: Delta Aggregate.
-- Usage: statistics inputs가 정상 세대에서 시작할 때 TEMP trigger로 설치.
-- Related: crates/kodstar-core/src/infrastructure/index/statistics/inputs.rs.

CREATE TEMP TRIGGER statistics_file_insert AFTER INSERT ON main.files_rows BEGIN
 UPDATE statistics_inputs SET scanned=scanned+1,
  analyzed=analyzed+(SELECT value='analyzed' FROM atoms WHERE id=NEW.status),
  skipped=skipped+(SELECT value='skipped' FROM atoms WHERE id=NEW.status),
  unsupported=unsupported+(SELECT value='unsupported' FROM atoms WHERE id=NEW.status),
  bytes=bytes+CASE WHEN (SELECT value FROM atoms WHERE id=NEW.status)='analyzed' THEN NEW.size ELSE 0 END;
END;
CREATE TEMP TRIGGER statistics_file_delete AFTER DELETE ON main.files_rows BEGIN
 UPDATE statistics_inputs SET scanned=scanned-1,
  analyzed=analyzed-(SELECT value='analyzed' FROM atoms WHERE id=OLD.status),
  skipped=skipped-(SELECT value='skipped' FROM atoms WHERE id=OLD.status),
  unsupported=unsupported-(SELECT value='unsupported' FROM atoms WHERE id=OLD.status),
  bytes=bytes-CASE WHEN (SELECT value FROM atoms WHERE id=OLD.status)='analyzed' THEN OLD.size ELSE 0 END;
END;
CREATE TEMP TRIGGER statistics_file_update AFTER UPDATE OF status,size ON main.files_rows
 WHEN OLD.status<>NEW.status OR OLD.size<>NEW.size BEGIN
 UPDATE statistics_inputs SET
  analyzed=analyzed+(SELECT value='analyzed' FROM atoms WHERE id=NEW.status)-(SELECT value='analyzed' FROM atoms WHERE id=OLD.status),
  skipped=skipped+(SELECT value='skipped' FROM atoms WHERE id=NEW.status)-(SELECT value='skipped' FROM atoms WHERE id=OLD.status),
  unsupported=unsupported+(SELECT value='unsupported' FROM atoms WHERE id=NEW.status)-(SELECT value='unsupported' FROM atoms WHERE id=OLD.status),
  bytes=bytes+CASE WHEN (SELECT value FROM atoms WHERE id=NEW.status)='analyzed' THEN NEW.size ELSE 0 END
    -CASE WHEN (SELECT value FROM atoms WHERE id=OLD.status)='analyzed' THEN OLD.size ELSE 0 END;
END;
CREATE TEMP TRIGGER statistics_symbol_insert AFTER INSERT ON main.symbols_rows BEGIN
 UPDATE statistics_inputs SET symbols=symbols+NOT EXISTS(
  SELECT 1 FROM symbols_rows WHERE identity=NEW.identity AND id<>NEW.id);
END;
CREATE TEMP TRIGGER statistics_symbol_delete AFTER DELETE ON main.symbols_rows BEGIN
 UPDATE statistics_inputs SET symbols=symbols-NOT EXISTS(
  SELECT 1 FROM symbols_rows WHERE identity=OLD.identity);
END;
CREATE TEMP TRIGGER statistics_symbol_update AFTER UPDATE OF syntax_key,resolved_key ON main.symbols_rows
 WHEN OLD.identity IS NOT NEW.identity BEGIN
 UPDATE statistics_inputs SET symbols=symbols-NOT EXISTS(
  SELECT 1 FROM symbols_rows WHERE identity=OLD.identity)+NOT EXISTS(
  SELECT 1 FROM symbols_rows WHERE identity=NEW.identity AND id<>NEW.id);
END;
