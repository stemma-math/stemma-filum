-- Agents run with their own defaults: no model or effort is chosen.
ALTER TABLE jobs DROP COLUMN model;
ALTER TABLE jobs DROP COLUMN effort;
ALTER TABLE machine_agents DROP COLUMN models;
ALTER TABLE machine_agents DROP COLUMN efforts;
