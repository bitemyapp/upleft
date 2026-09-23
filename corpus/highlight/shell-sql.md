# Shell and SQL

```bash
#!/usr/bin/env bash
set -euo pipefail
# comment at line start
readonly LOG_DIR="${HOME}/logs"   # trailing comment
for file in "$@"; do
  if [[ -f "$file" && ! -L $file ]]; then
    echo "Processing ${file##*/} ($((count + 1)))" >&2
    count=$((count+1)); url=http://x/#anchor; y=${x#prefix}
  elif [ -d "$file" ]; then cd "$file" || exit 1; fi
done
case "$1" in start) exec "$0" ;; *) printf '%s\n' 'literal $not_var \n' ;; esac
function cleanup() { local status=$?; trap - EXIT; return $status; }
echo $$ $PPID $1 ${#array[@]} `date` $(pwd) true false
cat <<EOF2
here doc $VAR
EOF2
```

```sh
export PATH=/usr/local/bin:$PATH && source ~/.profile; kill -9 %1 | wait
```

```console
$ cargo build --release
   Compiling upleft v0.1.0
```

```sql
-- A query
SELECT u.id, u.name AS "user name", COUNT(*) AS total
FROM users u
LEFT OUTER JOIN orders o ON o.user_id = u.id
WHERE u.created_at >= CURRENT_DATE - INTERVAL '30 days'
  AND u.email LIKE '%@example.com' AND o.total > 1.5e2
GROUP BY u.id, u.name
HAVING COUNT(*) > 10
ORDER BY total DESC NULLS LAST
LIMIT 50 OFFSET 0;
/* block comment */
CREATE TABLE IF NOT EXISTS t (id serial PRIMARY KEY, body text NOT NULL, doc jsonb, ts timestamptz);
INSERT INTO t (body) VALUES ('it''s escaped'), ("double");
select lower(name), coalesce(x, null) from t where flag = true;
```

```postgresql
WITH RECURSIVE r AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 10) SELECT * FROM r;
```
