// The real binary, on a real repository, for the browser to talk to.

import { spawn, type ChildProcess } from 'node:child_process';
import { Agent, BINARY, halt } from './agent.ts';
import { SLUG, build, env, type Fixture } from './fixture.ts';

export interface Running {
  url: string;
  fixture: Fixture;
  /** The agent of the course: a script that runs the commands of the binary. */
  agent: Agent;
  /**
   * Kill the server and the waits of the agent, and throw the repository
   * away once they are gone: a server that shuts down still writes there.
   */
  stop(): Promise<void>;
}

/**
 * Start qlearn on a fresh fixture and wait for the address it prints.
 *
 * The token is read from that line rather than forced in: the test walks the
 * same path as a learner.
 */
export async function start(binary = BINARY): Promise<Running> {
  const fixture = build();
  const child = spawn(binary, [SLUG, '--no-open', '--port', '0'], {
    cwd: fixture.repo,
    env: env({ NO_COLOR: '1' }),
  });

  let url: string;
  try {
    url = await address(child);
  } catch (e) {
    await halt(child);
    fixture.remove();
    throw e;
  }
  const agent = new Agent(fixture);

  return {
    url,
    fixture,
    agent,
    stop: async () => {
      await Promise.all([agent.stop(), halt(child)]);
      fixture.remove();
    },
  };
}

/** The address that the server prints once it listens. */
function address(child: ChildProcess): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    let seen = '';
    const timer = setTimeout(() => reject(new Error(`qlearn said nothing:\n${seen}`)), 20000);

    child.stdout?.on('data', (chunk) => {
      seen += String(chunk);
      const found = seen.match(/http:\/\/127\.0\.0\.1:\d+\/\?t=[a-f0-9]+/);
      if (found) {
        clearTimeout(timer);
        resolve(found[0]);
      }
    });
    child.stderr?.on('data', (chunk) => {
      seen += String(chunk);
    });
    // Not 'exit': it can come before the last of stderr, which says why.
    child.on('close', (code) => {
      clearTimeout(timer);
      reject(new Error(`qlearn stopped with ${code}:\n${seen}`));
    });
    child.on('error', (error) => {
      clearTimeout(timer);
      reject(new Error(`cannot run ${child.spawnfile} (run make build): ${error.message}`));
    });
  });
}
