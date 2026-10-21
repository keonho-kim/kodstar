-- Purpose: Scope and namespace binding selection.
-- Pattern: SQLite Query.
-- Usage: prepared.rs compiles this fixed statement once per generation.
-- Related: crates/kodstar-core/src/application/analysis/resolve/prepared.rs.

SELECT scope_id,target_local,module_name,imported_name,type_name,kind,start_byte
FROM bindings
WHERE file_id=?1
  AND scope_id=?2
  AND (name_key=?3 OR (?5='php' AND ?6 IN ('type','function') AND name_key=?3 COLLATE NOCASE) OR (?6='macro'
  AND name_key=?7
  AND kind IN ('import','export')))
  AND (?6='any' OR namespace IN ('any',?6))
  AND kind<>'owner_hint'
  AND (?5<>'elixir' OR kind<>'elixir_import_exclusion')
  AND (?5<>'php' OR (kind<>'php_method' AND kind<>'php_trait_alias' AND kind<>'php_trait_exclusion'))
  AND (?5<>'commonlisp' OR kind IN ('lisp_local_function','local','parameter','parameter_default','alias'))
  AND (?5<>'go' OR kind<>'field')
  AND (?8=0 OR kind NOT IN ('local','parameter','parameter_default','call_result','alias') OR NOT EXISTS(SELECT 1 FROM symbols s WHERE s.file_id=?1 AND s.local_id=?2 AND s.kind IN ('class','module','file')))
  AND (?5 NOT IN ('javascript','typescript') OR kind NOT IN ('export','export_type','commonjs_export_module','commonjs_export_exports'))
ORDER BY CASE WHEN start_byte<=?4 OR kind IN ('function','lisp_local_function','type','import','export','rust_constructor','value_constructor','value','razor_injection') THEN 0 ELSE 1 END,CASE WHEN name_key=?3 THEN 0 ELSE 1 END,start_byte DESC,id DESC LIMIT 1
