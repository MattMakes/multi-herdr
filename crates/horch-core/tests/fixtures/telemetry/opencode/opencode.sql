-- OpenCode 1.18.33 schema subset. Synthetic rows.
PRAGMA journal_mode=WAL;
CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);
CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES session(id), time_created INTEGER, time_updated INTEGER, data TEXT NOT NULL);
CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT REFERENCES message(id), session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
CREATE INDEX message_session_time_created_id_idx ON message (session_id, time_created, id);
INSERT INTO session VALUES ('ses_beta1','prj_1','/work/beta','SENTINEL-CONTENT title',1790599990000,1790600060000);
INSERT INTO session VALUES ('ses_other','prj_2','/elsewhere','not in any ledger',1790599990000,1790600060000);
INSERT INTO message VALUES ('msg_u1','ses_beta1',1790599995000,1790599995000,'{"role":"user","time":{"created":1790599995000},"summary":{"title":"SENTINEL-CONTENT"}}');
INSERT INTO message VALUES ('msg_o1','ses_beta1',1790600000000,1790600005000,'{"role":"assistant","parentID":"msg_u1","modelID":"nemotron-3-ultra-free","providerID":"opencode","mode":"build","cost":0,"tokens":{"total":1300,"input":1000,"output":300,"reasoning":40,"cache":{"read":0,"write":0}},"time":{"created":1790600000000,"completed":1790600005000}}');
INSERT INTO message VALUES ('msg_o2','ses_beta1',1790600006000,1790600010000,'{"role":"assistant","parentID":"msg_u1","modelID":"nemotron-3-ultra-free","providerID":"opencode","mode":"build","cost":0,"tokens":{"total":55,"input":50,"output":5,"reasoning":0,"cache":{"read":0,"write":0}},"time":{"created":1790600006000}}');
INSERT INTO message VALUES ('msg_o4','ses_beta1',1790600020000,1790600030000,'{"role":"assistant","parentID":"msg_u1","modelID":"nemotron-3-ultra-free","providerID":"opencode","mode":"build","cost":0,"tokens":{"total":800,"input":500,"output":100,"reasoning":0,"cache":{"read":200,"write":0}},"time":{"created":1790600020000,"completed":1790600030000}}');
INSERT INTO message VALUES ('msg_x1','ses_other',1790600000000,1790600005000,'{"role":"assistant","parentID":"msg_u1","modelID":"nemotron-3-ultra-free","providerID":"opencode","mode":"build","cost":0,"tokens":{"total":15554,"input":7777,"output":7777,"reasoning":0,"cache":{"read":0,"write":0}},"time":{"created":1790600000000,"completed":1790600005000}}');
INSERT INTO part VALUES ('prt_1','msg_o1','ses_beta1',1790600005000,1790600005000,'{"type":"step-finish","tokens":{"input":1000,"output":300,"reasoning":40,"cache":{"read":0,"write":0}},"cost":0}');
