PRAGMA foreign_keys=ON;
CREATE TABLE state(version INTEGER NOT NULL) STRICT;
INSERT INTO state VALUES(1);
CREATE TABLE documents(id INTEGER PRIMARY KEY,source TEXT NOT NULL) STRICT;
CREATE TABLE revisions(id INTEGER PRIMARY KEY,document INTEGER NOT NULL REFERENCES documents(id),number INTEGER NOT NULL,text TEXT NOT NULL,digest TEXT NOT NULL) STRICT;
CREATE TABLE passages(id INTEGER PRIMARY KEY,revision INTEGER NOT NULL REFERENCES revisions(id),start INTEGER NOT NULL,end INTEGER NOT NULL,text TEXT NOT NULL) STRICT;
CREATE TABLE entities(id INTEGER PRIMARY KEY,name TEXT NOT NULL) STRICT;
CREATE TABLE types(id INTEGER PRIMARY KEY,name TEXT NOT NULL) STRICT;
CREATE TABLE predicates(id INTEGER PRIMARY KEY,name TEXT NOT NULL) STRICT;
CREATE TABLE knowledge(id INTEGER PRIMARY KEY,kind TEXT NOT NULL,subject INTEGER NOT NULL REFERENCES entities(id),vocab INTEGER NOT NULL,object_entity INTEGER REFERENCES entities(id),literal TEXT,literal_kind TEXT,withdrawn INTEGER NOT NULL) STRICT;
CREATE TABLE evidence(knowledge INTEGER NOT NULL REFERENCES knowledge(id),passage INTEGER NOT NULL REFERENCES passages(id),PRIMARY KEY(knowledge,passage)) STRICT;
CREATE TABLE deferred_parent(id INTEGER PRIMARY KEY) STRICT;
CREATE TABLE deferred_child(id INTEGER REFERENCES deferred_parent(id) DEFERRABLE INITIALLY DEFERRED) STRICT;
INSERT INTO documents VALUES(1,'synthetic:atlas');
INSERT INTO entities VALUES(1,'Atlas'),(2,'Roadmap'),(99,'Unused');
INSERT INTO types VALUES(1,'Project'),(2,'Plan'),(3,'Unused');
INSERT INTO predicates VALUES(1,'depends_on'),(2,'minimum'),(3,'maximum'),(4,'enabled'),(5,'occurred'),(6,'description'),(99,'Unused');
INSERT INTO knowledge VALUES
 (1,'type_membership',1,1,NULL,NULL,NULL,0),
 (2,'type_membership',2,2,NULL,NULL,NULL,0),
 (3,'fact',1,1,2,NULL,NULL,0),
 (4,'fact',1,1,2,NULL,NULL,0),
 (5,'fact',1,2,NULL,'-9223372036854775808','integer',0),
 (6,'fact',1,3,NULL,'9223372036854775807','integer',0),
 (7,'fact',1,4,NULL,'true','boolean',0),
 (8,'fact',1,5,NULL,'"2026-09-28T00:00:00Z"','timestamp',0),
 (9,'fact',1,6,NULL,'"café"','string',0),
 (10,'fact',1,99,2,NULL,NULL,1);
