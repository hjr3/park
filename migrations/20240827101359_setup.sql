create table if not exists requests (
     request_id blob primary key,
     har blob not null,
     created_at integer not null
);
create index request_created on requests(created_at);
