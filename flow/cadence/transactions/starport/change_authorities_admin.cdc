// Update new authorities addresses
import Starport from 0xc8873a26b148ed14

transaction(authorities: [String]) {
     let admin: &Starport.Administrator

    prepare(signer: AuthAccount) {

        self.admin = signer.borrow<&Starport.Administrator>(from: /storage/admin)
            ?? panic("Could not borrow reference to storage Starport Participant")
    }

    execute {

        self.admin.changeAuthorities(newAuthorities: authorities)
    }
}