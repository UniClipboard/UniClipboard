
db-migration:
	export DATABASE_URL=uniclipboard.db && diesel migration generate $(name)

db-upgrade:
	export DATABASE_URL=uniclipboard.db && diesel migration run && diesel print-schema > src/db/schema.rs