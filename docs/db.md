# Database setup

## macos

### Install Postgres
```
brew install postgresql@14
brew services restart postgresql@14
```

### Create and configure user and db

```
$ createuser orduser
$ createdb ordscanner
$ psql --username=$(whoami) --dbname=ordscanner
ordscanner=> alter user orduser with encrypted password 'testtest';
ALTER ROLE
ordscanner=> grant all privileges on database ordscanner to orduser;
GRANT
```

### Reset the db

```
$ psql
$-# \c ordscanner
ordscanner=#TRUNCATE TABLE inscription_records;
```
