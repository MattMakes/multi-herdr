-- The OpenCode `message` table, cut to the columns the context reader reads.
-- Session ses_pickle: 1 finished assistant row (17,009), then 1 unfinished one.
-- Session ses_post: 1 finished row (30,000), a compaction summary row, then 1
-- finished row (9,500).
CREATE TABLE message (
  id text PRIMARY KEY,
  session_id text NOT NULL,
  time_updated integer NOT NULL,
  data text NOT NULL
);
INSERT INTO message VALUES ('msg_p0', 'ses_pickle', 1789902600000,
  '{"role":"user","time":{"created":1789902600000}}');
INSERT INTO message VALUES ('msg_p1', 'ses_pickle', 1789902700000,
  '{"role":"assistant","mode":"build","providerID":"opencode","modelID":"big-pickle","tokens":{"total":17009,"input":2000,"output":9,"reasoning":0,"cache":{"write":0,"read":15000}},"time":{"created":1789902610000,"completed":1789902700000},"finish":"stop"}');
INSERT INTO message VALUES ('msg_p2', 'ses_pickle', 1789902800000,
  '{"role":"assistant","mode":"build","providerID":"opencode","modelID":"big-pickle","tokens":{"total":0,"input":0,"output":0,"reasoning":0,"cache":{"write":0,"read":0}},"time":{"created":1789902790000}}');
INSERT INTO message VALUES ('msg_q1', 'ses_post', 1789903000000,
  '{"role":"assistant","mode":"build","providerID":"opencode","modelID":"big-pickle","tokens":{"total":30000,"input":5000,"output":0,"reasoning":0,"cache":{"write":0,"read":25000}},"time":{"created":1789902900000,"completed":1789903000000},"finish":"stop"}');
INSERT INTO message VALUES ('msg_q2', 'ses_post', 1789903100000,
  '{"role":"assistant","mode":"compaction","summary":true,"providerID":"opencode","modelID":"big-pickle","tokens":{"total":31200,"input":30000,"output":1200,"reasoning":0,"cache":{"write":0,"read":0}},"time":{"created":1789903050000,"completed":1789903100000},"finish":"stop"}');
INSERT INTO message VALUES ('msg_q3', 'ses_post', 1789903200000,
  '{"role":"assistant","mode":"build","providerID":"opencode","modelID":"big-pickle","tokens":{"total":0,"input":1500,"output":500,"reasoning":0,"cache":{"write":0,"read":7500}},"time":{"created":1789903150000,"completed":1789903200000},"finish":"stop"}');
