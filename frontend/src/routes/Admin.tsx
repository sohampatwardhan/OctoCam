import { useState, type FormEvent } from "react"
import { Loader2, Trash2, UserPlus } from "lucide-react"
import { useMe } from "@/hooks/useAuth"
import { friendlyUserError, useAddUser, useDeleteUser, useUsers } from "@/hooks/useUsers"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Select } from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import type { UserDto } from "@/lib/api"

function RoleBadge({ role }: { role: string }) {
  return (
    <Badge variant={role === "admin" ? "default" : "secondary"} className="capitalize">
      {role}
    </Badge>
  )
}

function AddUserForm() {
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [role, setRole] = useState("viewer")
  const [justAdded, setJustAdded] = useState(false)
  const addUser = useAddUser()

  const canSubmit = username.trim().length > 0 && password.length > 0

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    if (!canSubmit) return
    setJustAdded(false)
    addUser.mutate(
      { username: username.trim(), password, role },
      {
        onSuccess: () => {
          setUsername("")
          setPassword("")
          setRole("viewer")
          setJustAdded(true)
        },
      }
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Add a user</CardTitle>
        <CardDescription>Create another account that can sign in to this device.</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-3" onSubmit={handleSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-username">Username</Label>
            <Input
              id="new-username"
              name="username"
              autoComplete="off"
              value={username}
              onChange={(event) => {
                setUsername(event.target.value)
                setJustAdded(false)
              }}
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-password">Password</Label>
            <Input
              id="new-password"
              name="password"
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(event) => {
                setPassword(event.target.value)
                setJustAdded(false)
              }}
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-role">Role</Label>
            <Select
              id="new-role"
              name="role"
              className="max-w-40"
              value={role}
              onChange={(event) => setRole(event.target.value)}
            >
              <option value="viewer">Viewer</option>
              <option value="admin">Admin</option>
            </Select>
          </div>

          {addUser.isError && (
            <p className="text-sm text-destructive">{friendlyUserError(addUser.error.message)}</p>
          )}
          {justAdded && !addUser.isError && (
            <p className="text-sm text-success">User added.</p>
          )}

          <div>
            <Button type="submit" disabled={!canSubmit || addUser.isPending}>
              {addUser.isPending ? <Loader2 className="animate-spin" /> : <UserPlus />}
              Add user
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  )
}

function UsersTable({ users, currentUsername }: { users: UserDto[]; currentUsername?: string }) {
  const [pendingDelete, setPendingDelete] = useState<UserDto | null>(null)
  const deleteUser = useDeleteUser()

  return (
    <>
      <div className="flex flex-col gap-2">
        {users.map((user) => {
          const isSelf = user.username === currentUsername
          return (
            <div
              key={user.id}
              className="flex flex-col gap-2 rounded-lg border border-border px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="flex flex-1 flex-col gap-1 overflow-hidden text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium">{user.username}</span>
                  <RoleBadge role={user.role} />
                  {isSelf && (
                    <span className="text-xs text-muted-foreground">(you)</span>
                  )}
                </div>
                <span className="text-xs text-muted-foreground">Created {user.created_at}</span>
              </div>
              {!isSelf && (
                <Button
                  type="button"
                  variant="destructive"
                  size="sm"
                  onClick={() => {
                    deleteUser.reset()
                    setPendingDelete(user)
                  }}
                  disabled={deleteUser.isPending}
                >
                  <Trash2 />
                  Delete
                </Button>
              )}
            </div>
          )
        })}
      </div>

      <Dialog
        open={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) {
            setPendingDelete(null)
            deleteUser.reset()
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete {pendingDelete?.username}?</DialogTitle>
            <DialogDescription>
              This removes their account and signs them out everywhere. This can't be undone.
            </DialogDescription>
          </DialogHeader>

          {deleteUser.isError && (
            <p className="text-sm text-destructive">{friendlyUserError(deleteUser.error.message)}</p>
          )}

          <DialogFooter>
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button
              variant="destructive"
              disabled={deleteUser.isPending}
              onClick={() => {
                if (!pendingDelete) return
                deleteUser.mutate(pendingDelete.id, { onSuccess: () => setPendingDelete(null) })
              }}
            >
              {deleteUser.isPending ? "Deleting…" : "Delete user"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

export default function Admin() {
  const { data: me } = useMe()
  const { data: users, isLoading, isError } = useUsers()

  return (
    <div className="flex flex-col gap-6 p-4 sm:p-6">
      <h1 className="font-heading text-2xl font-semibold tracking-tight">Admin</h1>

      <Card>
        <CardHeader>
          <CardTitle>User accounts</CardTitle>
          <CardDescription>Everyone who can sign in to this device, and what they can do.</CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="flex flex-col gap-2">
              <Skeleton className="h-16 w-full rounded-lg" />
              <Skeleton className="h-16 w-full rounded-lg" />
            </div>
          ) : isError || !users ? (
            <p className="text-sm text-destructive">Couldn't load user accounts.</p>
          ) : users.length === 0 ? (
            <p className="text-sm text-muted-foreground">No user accounts found.</p>
          ) : (
            <UsersTable users={users} currentUsername={me?.username} />
          )}
        </CardContent>
      </Card>

      <AddUserForm />
    </div>
  )
}
