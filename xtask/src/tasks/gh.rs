//! [`GitHub`] through the `gh` CLI (already authenticated on the maintainer's machine). Developer
//! tooling only: the Dalica app never touches the network for this.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;
use serde_json::Value;

use super::config::{OWNER, PROJECT, REPO};
use super::github::{GitHub, Issue, IssueEdit, Item, MilestoneInfo, NewIssue, ProjectFields, PullRequest, Status, TextField};

#[derive(Default)]
pub struct GhCli {
    /// Project node id, Status field id and option ids, text field ids; read once.
    ids: Mutex<Option<ProjectIds>>,
}

#[derive(Clone, Default)]
struct ProjectIds {
    project: String,
    status_field: String,
    options: Vec<(String, String)>,
    text_fields: Vec<(String, String)>,
}

fn gh(args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("gh");
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd.spawn().map_err(|e| format!("cannot run gh: {e}"))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input.as_bytes()).map_err(|e| format!("cannot write to gh: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("gh failed: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        Err(format!("gh {} failed: {}", args.first().copied().unwrap_or(""), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| format!("cannot parse gh output: {e}"))
}

/// `https://github.com/o/r/issues/12` → 12.
fn trailing_number(url: &str) -> Result<u64, String> {
    url.trim().rsplit('/').next().and_then(|n| n.parse().ok()).ok_or_else(|| format!("unexpected gh output `{url}`"))
}

/// The board's items in board order, 100 per page, with only the fields `xtask` reads (issue number,
/// Status, Agent, Branch). GitHub prices a query by the nodes it could return: this costs 1 point
/// per page, where `gh project item-list --limit 2000` (every field of every item) cost about 100.
const ITEMS_QUERY: &str = "query($owner:String!,$project:Int!,$after:String){user(login:$owner){projectV2(number:$project){\
items(first:100,after:$after,orderBy:{field:POSITION,direction:ASC}){pageInfo{hasNextPage endCursor}\
nodes{id content{...on Issue{number}}\
status:fieldValueByName(name:\"Status\"){...on ProjectV2ItemFieldSingleSelectValue{name}}\
agent:fieldValueByName(name:\"Agent\"){...on ProjectV2ItemFieldTextValue{text}}\
branch:fieldValueByName(name:\"Branch\"){...on ProjectV2ItemFieldTextValue{text}}}}}}}";

/// Pages read before giving up (a bounded loop: 100 pages = 10,000 items).
const ITEM_PAGES: usize = 100;

/// One page of [`ITEMS_QUERY`]: its items (drafts and pull requests, which have no issue number,
/// are skipped) and the cursor of the next page, if any.
fn parse_items_page(v: &Value) -> Result<(Vec<Item>, Option<String>), String> {
    let conn = &v["data"]["user"]["projectV2"]["items"];
    let nodes = conn["nodes"].as_array().ok_or_else(|| format!("unexpected project items reply: {}", short(v)))?;
    let items = nodes
        .iter()
        .filter_map(|i| {
            Some(Item {
                id: i["id"].as_str()?.to_string(),
                issue: i["content"]["number"].as_u64()?,
                status: i["status"]["name"].as_str().and_then(Status::from_name),
                agent: i["agent"]["text"].as_str().unwrap_or("").to_string(),
                branch: i["branch"]["text"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();
    let next =
        (conn["pageInfo"]["hasNextPage"].as_bool() == Some(true)).then(|| conn["pageInfo"]["endCursor"].as_str().map(str::to_string)).flatten();
    Ok((items, next))
}

/// The start of a reply, for error messages.
fn short(v: &Value) -> String {
    v.to_string().chars().take(200).collect()
}

#[derive(Deserialize)]
struct RawIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    state: String,
    #[serde(default)]
    labels: Vec<Named>,
    #[serde(default)]
    assignees: Vec<Login>,
    #[serde(default)]
    milestone: Option<Titled>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct Titled {
    title: String,
}

impl GhCli {
    fn ids(&self) -> Result<ProjectIds, String> {
        let mut guard = self.ids.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(ids) = guard.as_ref() {
            return Ok(ids.clone());
        }
        let project = json(&gh(&["project", "view", &PROJECT.to_string(), "--owner", OWNER, "--format", "json"], None)?)?;
        let mut ids = ProjectIds { project: project["id"].as_str().unwrap_or_default().to_string(), ..ProjectIds::default() };
        let fields = json(&gh(&["project", "field-list", &PROJECT.to_string(), "--owner", OWNER, "--format", "json", "--limit", "100"], None)?)?;
        for f in fields["fields"].as_array().into_iter().flatten() {
            let (name, id, kind) = (f["name"].as_str().unwrap_or(""), f["id"].as_str().unwrap_or(""), f["type"].as_str().unwrap_or(""));
            if name == "Status" {
                ids.status_field = id.to_string();
                for o in f["options"].as_array().into_iter().flatten() {
                    ids.options.push((o["name"].as_str().unwrap_or("").to_string(), o["id"].as_str().unwrap_or("").to_string()));
                }
            } else if kind == "ProjectV2Field" {
                ids.text_fields.push((name.to_string(), id.to_string()));
            }
        }
        if ids.project.is_empty() || ids.status_field.is_empty() {
            return Err("cannot find project 3 or its Status field; run `cargo xtask tasks setup`".into());
        }
        *guard = Some(ids.clone());
        Ok(ids)
    }

    fn item_edit(&self, item: &str, field: &str, value_args: &[&str]) -> Result<(), String> {
        let ids = self.ids()?;
        let mut args = vec!["project", "item-edit", "--id", item, "--project-id", ids.project.as_str(), "--field-id", field];
        args.extend_from_slice(value_args);
        gh(&args, None).map(|_| ())
    }
}

impl GitHub for GhCli {
    fn issues(&self) -> Result<Vec<Issue>, String> {
        let out = gh(
            &["issue", "list", "--repo", REPO, "--state", "all", "--limit", "2000", "--json", "number,title,body,state,labels,assignees,milestone"],
            None,
        )?;
        let raw: Vec<RawIssue> = serde_json::from_str(&out).map_err(|e| format!("cannot parse gh issue list: {e}"))?;
        Ok(raw
            .into_iter()
            .map(|r| Issue {
                number: r.number,
                title: r.title,
                body: r.body,
                open: r.state.eq_ignore_ascii_case("open"),
                labels: r.labels.into_iter().map(|l| l.name).collect(),
                assignees: r.assignees.into_iter().map(|a| a.login).collect(),
                milestone: r.milestone.map(|m| m.title),
            })
            .collect())
    }

    fn create_issue(&self, issue: &NewIssue) -> Result<u64, String> {
        let mut args = vec!["issue", "create", "--repo", REPO, "--title", issue.title.as_str(), "--body-file", "-"];
        for l in &issue.labels {
            args.extend(["--label", l.as_str()]);
        }
        if let Some(m) = &issue.milestone {
            args.extend(["--milestone", m.as_str()]);
        }
        if let Some(a) = &issue.assignee {
            args.extend(["--assignee", a.as_str()]);
        }
        trailing_number(&gh(&args, Some(&issue.body))?)
    }

    fn edit_issue(&self, number: u64, edit: &IssueEdit) -> Result<(), String> {
        let n = number.to_string();
        let mut args = vec!["issue", "edit", n.as_str(), "--repo", REPO];
        if let Some(t) = &edit.title {
            args.extend(["--title", t.as_str()]);
        }
        if edit.body.is_some() {
            args.extend(["--body-file", "-"]);
        }
        for l in &edit.add_labels {
            args.extend(["--add-label", l.as_str()]);
        }
        for l in &edit.remove_labels {
            args.extend(["--remove-label", l.as_str()]);
        }
        if let Some(m) = &edit.milestone {
            args.extend(["--milestone", m.as_str()]);
        }
        if let Some(a) = &edit.add_assignee {
            args.extend(["--add-assignee", a.as_str()]);
        }
        gh(&args, edit.body.as_deref()).map(|_| ())
    }

    fn comment(&self, number: u64, body: &str) -> Result<(), String> {
        gh(&["issue", "comment", &number.to_string(), "--repo", REPO, "--body-file", "-"], Some(body)).map(|_| ())
    }

    fn close_issue(&self, number: u64, comment: &str) -> Result<(), String> {
        gh(&["issue", "close", &number.to_string(), "--repo", REPO, "--comment", comment], None).map(|_| ())
    }

    fn items(&self) -> Result<Vec<Item>, String> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..ITEM_PAGES {
            let owner = format!("owner={OWNER}");
            let project = format!("project={PROJECT}");
            let q = format!("query={ITEMS_QUERY}");
            let mut args = vec!["api", "graphql", "-f", q.as_str(), "-f", owner.as_str(), "-F", project.as_str()];
            let after;
            if let Some(c) = &cursor {
                after = format!("after={c}");
                args.extend(["-f", after.as_str()]);
            }
            let (page, next) = parse_items_page(&json(&gh(&args, None)?)?)?;
            items.extend(page);
            match next {
                Some(c) => cursor = Some(c),
                None => return Ok(items),
            }
        }
        Err(format!("the project has more than {} items; raise ITEM_PAGES", ITEM_PAGES * 100))
    }

    fn add_item(&self, issue: u64) -> Result<String, String> {
        let url = format!("https://github.com/{REPO}/issues/{issue}");
        let out = gh(&["project", "item-add", &PROJECT.to_string(), "--owner", OWNER, "--url", &url, "--format", "json"], None)?;
        json(&out)?["id"].as_str().map(str::to_string).ok_or_else(|| "gh project item-add returned no id".into())
    }

    fn set_status(&self, item: &str, status: Status) -> Result<(), String> {
        let ids = self.ids()?;
        let option = ids
            .options
            .iter()
            .find(|(n, _)| n == status.name())
            .map(|(_, id)| id.clone())
            .ok_or_else(|| format!("the project's Status field has no `{status}` option; run `cargo xtask tasks setup`"))?;
        self.item_edit(item, &ids.status_field, &["--single-select-option-id", &option])
    }

    fn set_field(&self, item: &str, field: TextField, value: &str) -> Result<(), String> {
        let ids = self.ids()?;
        let field_id = ids
            .text_fields
            .iter()
            .find(|(n, _)| n == field.name())
            .map(|(_, id)| id.clone())
            .ok_or_else(|| format!("the project has no `{}` field; run `cargo xtask tasks setup`", field.name()))?;
        if value.is_empty() { self.item_edit(item, &field_id, &["--clear"]) } else { self.item_edit(item, &field_id, &["--text", value]) }
    }

    fn move_item(&self, item: &str, after: Option<&str>) -> Result<(), String> {
        let ids = self.ids()?;
        let query = "mutation($project:ID!,$item:ID!,$after:ID){updateProjectV2ItemPosition(input:{projectId:$project,itemId:$item,afterId:$after}){clientMutationId}}";
        let q = format!("query={query}");
        let p = format!("project={}", ids.project);
        let i = format!("item={item}");
        let mut args = vec!["api", "graphql", "-f", q.as_str(), "-f", p.as_str(), "-f", i.as_str()];
        let a;
        if let Some(after) = after {
            a = format!("after={after}");
            args.extend(["-f", a.as_str()]);
        }
        gh(&args, None).map(|_| ())
    }

    fn milestones(&self) -> Result<Vec<MilestoneInfo>, String> {
        let out = gh(&["api", "--paginate", &format!("repos/{REPO}/milestones?state=all&per_page=100"), "--jq", ".[] | {title, description}"], None)?;
        out.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let v = json(l)?;
                Ok(MilestoneInfo {
                    title: v["title"].as_str().unwrap_or("").to_string(),
                    description: v["description"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect()
    }

    fn create_milestone(&self, title: &str, description: &str) -> Result<(), String> {
        let t = format!("title={title}");
        let d = format!("description={description}");
        gh(&["api", &format!("repos/{REPO}/milestones"), "-f", &t, "-f", &d], None).map(|_| ())
    }

    fn labels(&self) -> Result<Vec<String>, String> {
        let out = gh(&["label", "list", "--repo", REPO, "--limit", "500", "--json", "name"], None)?;
        let raw: Vec<Named> = serde_json::from_str(&out).map_err(|e| format!("cannot parse gh label list: {e}"))?;
        Ok(raw.into_iter().map(|l| l.name).collect())
    }

    fn create_label(&self, name: &str, color: &str, description: &str) -> Result<(), String> {
        gh(&["label", "create", name, "--repo", REPO, "--color", color, "--description", description], None).map(|_| ())
    }

    fn project_fields(&self) -> Result<ProjectFields, String> {
        let ids = self.ids()?;
        Ok(ProjectFields {
            status_options: ids.options.iter().map(|(n, _)| n.clone()).collect(),
            text_fields: ids.text_fields.iter().map(|(n, _)| n.clone()).collect(),
        })
    }

    fn create_text_field(&self, name: &str) -> Result<(), String> {
        gh(&["project", "field-create", &PROJECT.to_string(), "--owner", OWNER, "--name", name, "--data-type", "TEXT"], None)?;
        *self.ids.lock().unwrap_or_else(PoisonError::into_inner) = None;
        Ok(())
    }

    fn find_pr(&self, branch: &str) -> Result<Option<PullRequest>, String> {
        let out = gh(&["pr", "list", "--repo", REPO, "--head", branch, "--state", "open", "--json", "number,url,labels"], None)?;
        let v = json(&out)?;
        Ok(v.as_array().and_then(|a| a.first()).and_then(|p| {
            let labels = p["labels"].as_array().into_iter().flatten().filter_map(|l| l["name"].as_str().map(str::to_string)).collect();
            Some(PullRequest { number: p["number"].as_u64()?, url: p["url"].as_str()?.to_string(), labels })
        }))
    }

    fn open_pr(&self, branch: &str, title: &str, body: &str) -> Result<PullRequest, String> {
        let url = gh(&["pr", "create", "--repo", REPO, "--head", branch, "--base", "main", "--title", title, "--body-file", "-"], Some(body))?;
        Ok(PullRequest { number: trailing_number(&url)?, url, labels: Vec::new() })
    }

    fn main_ci_failure(&self) -> Result<Option<String>, String> {
        let out = gh(
            &["run", "list", "--repo", REPO, "--branch", "main", "--workflow", "CI", "--limit", "10", "--json", "status,conclusion,displayTitle,url"],
            None,
        )?;
        let runs: Value = serde_json::from_str(&out).map_err(|e| format!("gh run list: {e}"))?;
        let finished = runs.as_array().and_then(|r| r.iter().find(|r| r["status"].as_str() == Some("completed")));
        Ok(finished
            .filter(|r| r["conclusion"].as_str() == Some("failure"))
            .map(|r| format!("{} {}", r["displayTitle"].as_str().unwrap_or("?"), r["url"].as_str().unwrap_or(""))))
    }

    fn squash_merge(&self, pr: u64, subject: &str) -> Result<(), String> {
        gh(&["pr", "merge", &pr.to_string(), "--repo", REPO, "--squash", "--subject", subject, "--delete-branch"], None).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_items_page_reads_status_agent_and_branch_and_skips_drafts() {
        let page = json(
            r#"{"data":{"user":{"projectV2":{"items":{
                "pageInfo":{"hasNextPage":true,"endCursor":"Y3Vy"},
                "nodes":[
                  {"id":"PVTI_1","content":{"number":43},"status":{"name":"In Progress"},"agent":{"text":"agent-3"},"branch":{"text":"task/M2.2-x"}},
                  {"id":"PVTI_2","content":{"number":44},"status":{"name":"Todo"},"agent":null,"branch":null},
                  {"id":"PVTI_3","content":{},"status":null,"agent":null,"branch":null}
                ]}}}}}"#,
        )
        .unwrap();
        let (items, next) = parse_items_page(&page).unwrap();
        assert_eq!(next.as_deref(), Some("Y3Vy"));
        assert_eq!(items.len(), 2, "the draft without an issue number is skipped");
        assert_eq!((items[0].issue, items[0].status, items[0].agent.as_str()), (43, Some(Status::InProgress), "agent-3"));
        assert_eq!(items[0].branch, "task/M2.2-x");
        assert_eq!((items[1].status, items[1].agent.as_str(), items[1].branch.as_str()), (Some(Status::Todo), "", ""));
    }

    #[test]
    fn the_last_page_has_no_cursor_and_an_error_reply_is_an_error() {
        let last = json(r#"{"data":{"user":{"projectV2":{"items":{"pageInfo":{"hasNextPage":false,"endCursor":"Y3Vy"},"nodes":[]}}}}}"#).unwrap();
        assert_eq!(parse_items_page(&last).unwrap(), (Vec::new(), None));
        let err = json(r#"{"errors":[{"message":"API rate limit already exceeded"}]}"#).unwrap();
        assert!(parse_items_page(&err).unwrap_err().contains("rate limit"));
    }

    #[test]
    fn the_query_asks_for_one_page_of_100_in_board_order() {
        assert!(ITEMS_QUERY.contains("items(first:100,after:$after,orderBy:{field:POSITION,direction:ASC})"));
        assert!(!ITEMS_QUERY.contains('\n'));
    }
}
